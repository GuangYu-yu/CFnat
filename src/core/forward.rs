use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{self, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::core::backend::Backend;
use crate::core::cancel::CancellationToken;
use crate::core::loadbalancer::LoadBalancer;
use crate::log::push_log;

const READABLE_TIMEOUT_SECS: u64 = 10;
const BUFFER_SIZE: usize = 128 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RETRY: usize = 3;

fn is_tls(buf: &[u8]) -> bool {
    !buf.is_empty() && buf[0] == 0x16
}

async fn transfer_direction(
    reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    record_metrics: Option<(Arc<LoadBalancer>, Arc<Backend>, Instant)>,
) -> io::Result<()> {
    if let Some((lb, backend, start)) = record_metrics {
        match tokio::time::timeout(
            Duration::from_secs(READABLE_TIMEOUT_SECS),
            reader.readable()
        ).await {
            Ok(Ok(_)) => {
                let delay = start.elapsed().as_secs_f32() * 1000.0;
                lb.record_delay(&backend, delay);
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("后端 {} 秒无响应", READABLE_TIMEOUT_SECS)
                ));
            }
        }
    }

    let mut buffered = BufReader::with_capacity(BUFFER_SIZE, reader);
    io::copy(&mut buffered, &mut writer).await?;
    Ok(())
}

/// 连接后端的合适端口，使用 peek 与投机连接并行策略。
/// tls_port != http_port 时，peek 与 TLS 端口连接同时进行；
/// tls_port == http_port 时，跳过 peek 直接连接。
async fn connect_backend(
    backend: &Backend,
    client: &TcpStream,
    tls_port: u16,
    http_port: u16,
) -> io::Result<TcpStream> {
    if tls_port == http_port {
        // 端口相同，无需 peek，直接连接
        let addr = SocketAddr::new(backend.addr.ip(), tls_port);
        let server = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "连接超时"))??;
        return Ok(server);
    }

    // peek 首字节与连接 TLS 端口同时进行
    let mut buf = [0u8; 1];
    let peek_fut = tokio::time::timeout(Duration::from_secs(5), client.peek(&mut buf));
    let tls_addr = SocketAddr::new(backend.addr.ip(), tls_port);
    let tls_connect_fut = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(tls_addr));

    let (peek_result, tls_result) = tokio::join!(peek_fut, tls_connect_fut);

    // 判断 peek 结果
    let is_tls_traffic = match peek_result {
        Ok(Ok(_)) => is_tls(&buf),
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "peek 超时"));
        }
    };

    if is_tls_traffic {
        // TLS 流量，使用已建立的 TLS 连接
        match tls_result {
            Ok(Ok(server)) => Ok(server),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "TLS 连接超时")),
        }
    } else {
        // HTTP 流量，关闭 TLS 连接，连接 HTTP 端口
        drop(tls_result);
        let http_addr = SocketAddr::new(backend.addr.ip(), http_port);
        let server = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(http_addr))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTP 连接超时"))??;
        Ok(server)
    }
}

async fn handle_client(
    client: TcpStream,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
) -> io::Result<()> {
    // 重试循环：连接失败时尝试下一个后端
    for attempt in 0..MAX_RETRY {
        let backend = match lb.select() {
            Some(b) => b,
            None => {
                if attempt == 0 {
                    push_log("WARN", "有客户端连接但无可用后端");
                }
                return Err(io::Error::new(io::ErrorKind::NotFound, "无可用后端"));
            }
        };

        match connect_backend(&backend, &client, tls_port, http_port).await {
            Ok(server) => {
                let start = Instant::now();

                server.set_nodelay(true)?;
                client.set_nodelay(true)?;

                let (client_read, client_write) = client.into_split();
                let (server_read, server_write) = server.into_split();

                let lb_inner = lb.clone();
                let backend_inner = backend.clone();

                let s2c = transfer_direction(
                    server_read,
                    client_write,
                    Some((lb_inner, backend_inner, start)),
                );

                let c2s = transfer_direction(client_read, server_write, None);

                let result = tokio::select! {
                    res = c2s => res,
                    res = s2c => res,
                };

                if result.is_err() {
                    lb.record_loss(&backend, true);
                } else {
                    lb.record_loss(&backend, false);
                }

                lb.check_and_evict(&backend);
                lb.release(&backend);
                return result;
            }
            Err(_) => {
                // 连接失败，记录丢包并释放，尝试下一个后端
                lb.record_loss(&backend, true);
                lb.check_and_evict(&backend);
                lb.release(&backend);
            }
        }
    }

    Err(io::Error::new(io::ErrorKind::NotConnected, "所有后端连接尝试均失败"))
}

pub async fn run_forward(
    addr: SocketAddr,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
    cancel_token: CancellationToken,
) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    push_log("INFO", &format!("转发服务 {} (TLS:{}, HTTP:{})", 
        addr, tls_port, http_port));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((client, _)) => {
                        let lb = lb.clone();
                        tokio::spawn(async move {
                            if let Err(_e) = handle_client(client, lb, tls_port, http_port).await {}
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(24) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            _ = cancel_token.cancelled() => {
                push_log("INFO", "[转发服务] 收到停止信号，退出");
                break;
            }
        }
    }

    Ok(())
}