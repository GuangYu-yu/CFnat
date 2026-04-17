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
) -> io::Result<()> {
    let mut buf = [0u8; 1];
    tokio::time::timeout(Duration::from_secs(5), client.peek(&mut buf))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "peek timed out"))??;

    let backend = lb.select().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "无可用后端")
    })?;

    let port = if is_tls(&buf) {
        tls_port
    } else {
        http_port
    };

    let target_addr = std::net::SocketAddr::new(backend.addr.ip(), port);

    let result = async {
        let server = TcpStream::connect(target_addr).await?;
        
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
        
        result
    }
    .await;
    
    if result.is_err() {
        lb.record_loss(&backend, true);
    } else {
        lb.record_loss(&backend, false);
    }

    lb.check_and_evict(&backend);

    lb.release(&backend);
    result?;

    Ok(())
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