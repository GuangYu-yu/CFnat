use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
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

async fn handle_client(
    client: TcpStream,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
    warned_no_backend: Arc<AtomicBool>,
) -> io::Result<()> {
    let connect_start = Instant::now();

    // 确定目标端口
    let target_port = if tls_port == http_port {
        tls_port
    } else {
        let mut buf = [0u8; 1];
        let is_tls = match tokio::time::timeout(Duration::from_secs(5), client.peek(&mut buf)).await {
            Ok(Ok(_)) => is_tls(&buf),
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "peek 超时")),
        };
        if is_tls { tls_port } else { http_port }
    };

    // 选取最多 2 个后端
    let backends: Vec<Arc<Backend>> = (0..2).filter_map(|_| lb.select()).collect();
    if backends.is_empty() {
        if !warned_no_backend.swap(true, Ordering::Relaxed) {
            push_log("WARN", "有客户端连接但无可用后端");
        }
        return Err(io::ErrorKind::NotConnected.into());
    }
    warned_no_backend.store(false, Ordering::Relaxed);

    // 并发连接竞速
    let mut race: tokio::task::JoinSet<io::Result<(Arc<Backend>, TcpStream)>> = tokio::task::JoinSet::new();
    for backend in &backends {
        let backend = backend.clone();
        let addr = SocketAddr::new(backend.addr.ip(), target_port);
        race.spawn(async move {
            let server = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "连接超时"))??;
            Ok((backend, server))
        });
    }

    // 等待第一个成功的连接
    let winner = loop {
        match race.join_next().await {
            Some(Ok(Ok((backend, server)))) => break Some((backend, server)),
            Some(Ok(Err(_))) => continue,
            Some(Err(_)) => continue,
            None => break None,
        }
    };
    race.shutdown().await;

    // 处理连接结果
    match winner {
        Some((ref backend, server)) => {
            // 释放未选中的后端
            for b in &backends {
                if !Arc::ptr_eq(b, backend) {
                    lb.release(b);
                }
            }

            // 配置连接
            server.set_nodelay(true)?;
            client.set_nodelay(true)?;

            // 转发数据
            let start = Instant::now();
            let (client_read, client_write) = client.into_split();
            let (server_read, server_write) = server.into_split();

            let result = tokio::select! {
                res = transfer_direction(server_read, client_write, Some((lb.clone(), backend.clone(), start))) => res,
                res = transfer_direction(client_read, server_write, None) => res,
            };

            lb.check_and_evict(backend);
            lb.release(backend);

            let lifetime = connect_start.elapsed();
            if result.is_err() && lifetime.as_millis() < 3000 {
                backend.record_fast_fail();
                if backend.get_fast_fail_count() >= 2 {
                    push_log("WARN", &format!("[-] {} 连续快速断开，移除", backend.addr));
                    backend.mark_removed();
                }
            } else if result.is_ok() {
                backend.reset_fast_fail();
            }

            result
        }
        None => {
            // 所有连接失败
            for backend in &backends {
                lb.record_loss(backend, true);
                lb.check_and_evict(backend);
                lb.release(backend);
            }
            Err(io::Error::new(io::ErrorKind::NotConnected, "全部连接失败"))
        }
    }
}

pub async fn run_forward(
    addr: SocketAddr,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
    cancel_token: CancellationToken,
) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let warned_no_backend = Arc::new(AtomicBool::new(false));

    push_log("INFO", &format!("转发服务 {} (TLS:{}, HTTP:{})",
        addr, tls_port, http_port));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((client, _)) => {
                        let lb = lb.clone();
                        let warned_no_backend = warned_no_backend.clone();
                        tokio::spawn(async move {
                            if let Err(_e) = handle_client(client, lb, tls_port, http_port, warned_no_backend).await {}
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