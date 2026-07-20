use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{self};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::core::backend::Backend;
use crate::core::cancel::CancellationToken;
use crate::core::loadbalancer::LoadBalancer;
use crate::log::push_log;

const READABLE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[inline]
fn is_tls(first_byte: u8) -> bool {
    first_byte == 0x16
}

async fn transfer_direction(
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    metrics: Option<(Arc<LoadBalancer>, Arc<Backend>, Instant)>,
) -> io::Result<()> {
    if let Some((lb, backend, start)) = metrics {
        tokio::time::timeout(READABLE_TIMEOUT, reader.readable())
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        lb.record(&backend, Some(start.elapsed().as_secs_f32() * 1000.0), false);
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let rfd = reader.as_raw_fd();
        let wfd = writer.as_raw_fd();
        tokio::task::spawn_blocking(move || {
            let _reader = reader;
            let _writer = writer;
            crate::core::splice::transfer(rfd, wfd)
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
    }

    #[cfg(not(target_os = "linux"))]
    tokio::time::timeout(IDLE_TIMEOUT, io::copy(&mut reader, &mut writer))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;

    Ok(())
}

async fn race_connect(
    backends: &[Arc<Backend>],
    port: u16,
    lb: &Arc<LoadBalancer>,
) -> Option<(Arc<Backend>, TcpStream)> {
    let mut set: tokio::task::JoinSet<std::io::Result<(Arc<Backend>, Option<TcpStream>)>> =
        tokio::task::JoinSet::new();
    for b in backends {
        let b = b.clone();
        let addr = SocketAddr::new(b.addr.ip(), port);
        let lb = lb.clone();
        set.spawn(async move {
            match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => Ok((b, Some(stream))),
                _ => {
                    lb.record(&b, None, true);
                    Ok((b, None))
                }
            }
        });
    }

    let mut winner = None;
    while let Some(res) = set.join_next().await {
        if let Ok(Ok((b, Some(stream)))) = res {
            winner = Some((b, stream));
            break;
        }
    }
    set.shutdown().await;
    winner
}

async fn handle_client(
    client: TcpStream,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
    warned: Arc<AtomicBool>,
) -> io::Result<()> {
    let connect_start = Instant::now();

    // 确定目标端口
    let target_port = if tls_port == http_port {
        tls_port
    } else {
        let mut buf = [0u8; 1];
        let tls = match tokio::time::timeout(Duration::from_secs(5), client.peek(&mut buf)).await {
            Ok(Ok(_)) => is_tls(buf[0]),
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::from(io::ErrorKind::TimedOut)),
        };
        if tls { tls_port } else { http_port }
    };

    // 选后端
    let backends: Vec<Arc<Backend>> = (0..2).filter_map(|_| lb.select()).collect();
    if backends.is_empty() {
        if !warned.swap(true, Ordering::Relaxed) {
            push_log("WARN", "有客户端连接但无可用后端");
        }
        return Err(io::ErrorKind::NotConnected.into());
    }
    warned.store(false, Ordering::Relaxed);

    // 并发竞速连接
    let winner = race_connect(&backends, target_port, &lb).await;

    let (backend, server) = match winner {
        Some(w) => w,
        None => {
            for b in &backends {
                lb.check_and_evict(b);
                lb.release(b);
            }
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
    };

    // 释放未选中的后端
    for b in &backends {
        if !Arc::ptr_eq(b, &backend) {
            lb.release(b);
        }
    }

    // 配置连接并转发
    server.set_nodelay(true)?;
    client.set_nodelay(true)?;

    let start = Instant::now();
    let (cr, cw) = client.into_split();
    let (sr, sw) = server.into_split();

    let c2s = tokio::spawn(transfer_direction(cr, sw, None));
    let s2c = tokio::spawn(transfer_direction(sr, cw, Some((lb.clone(), backend.clone(), start))));
    let (r1, r2) = tokio::join!(c2s, s2c);

    let result = match (r1, r2) {
        (Ok(Ok(())), Ok(Ok(()))) => Ok(()),
        (Ok(Err(e)), _) | (_, Ok(Err(e))) => Err(e),
        (Err(_), _) | (_, Err(_)) => Err(io::Error::from(io::ErrorKind::Other)),
    };

    lb.check_and_evict(&backend);
    lb.release(&backend);

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

pub async fn run_forward(
    addr: SocketAddr,
    lb: Arc<LoadBalancer>,
    tls_port: u16,
    http_port: u16,
    cancel_token: CancellationToken,
) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let warned = Arc::new(AtomicBool::new(false));
    push_log("INFO", &format!("转发服务 {} (TLS:{}, HTTP:{})", addr, tls_port, http_port));

    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((client, _)) => {
                        let lb = lb.clone();
                        let warned = warned.clone();
                        tokio::spawn(async move {
                            let _ = handle_client(client, lb, tls_port, http_port, warned).await;
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(24) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
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