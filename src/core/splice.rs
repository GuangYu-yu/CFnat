use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};

const BUF_SIZE: usize = 64 * 1024;

struct Pipe {
    reader: OwnedFd,
    writer: OwnedFd,
}

impl Pipe {
    fn new() -> io::Result<Self> {
        let mut fds = [-1i32; 2];
        // SAFETY: pipe2 调用使用正确大小的缓冲区(fds)和标准标志 O_CLOEXEC。
        // 返回值已检查错误，fd 所有权由 OwnedFd 接管。
        let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            // SAFETY: fds[0] 和 fds[1] 是 pipe2 刚返回的有效文件描述符。
            // OwnedFd::from_raw_fd 接管所有权，防止重复关闭。
            reader: unsafe { OwnedFd::from_raw_fd(fds[0]) },
            writer: unsafe { OwnedFd::from_raw_fd(fds[1]) },
        })
    }
}

pub fn transfer(reader_fd: RawFd, writer_fd: RawFd) -> io::Result<()> {
    let pipe = Pipe::new()?;
    splice_loop(
        reader_fd,
        writer_fd,
        pipe.reader.as_raw_fd(),
        pipe.writer.as_raw_fd(),
    )
}

fn splice_once(src: RawFd, dst: RawFd, len: usize) -> io::Result<usize> {
    loop {
        // SAFETY: src 和 dst 是本进程拥有的有效文件描述符。
        // 偏移量传空指针适用于管道/套接字（没有文件偏移概念）。
        let n = unsafe {
            libc::splice(src, std::ptr::null_mut(), dst, std::ptr::null_mut(), len, 0)
        };
        if n >= 0 {
            return Ok(n as usize);
        }
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(err);
    }
}

fn poll_wait(fd: RawFd, events: i16) -> io::Result<()> {
    let mut pfd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    loop {
        // SAFETY: pfd 指向单个合法 pollfd，nfds=1 与之匹配；
        // fd 由调用方保证有效，timeout=-1 为阻塞等待。
        let ret = unsafe { libc::poll(&mut pfd, 1, -1) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        break;
    }
    if pfd.revents & events != 0 {
        return Ok(());
    }
    Err(io::Error::from(io::ErrorKind::ConnectionReset))
}

fn splice_loop(
    input_fd: RawFd,
    output_fd: RawFd,
    pipe_reader: RawFd,
    pipe_writer: RawFd,
) -> io::Result<()> {
    loop {
        let n = loop {
            match splice_once(input_fd, pipe_writer, BUF_SIZE) {
                Ok(0) => {
                    // SAFETY: output_fd 是调用方传入的有效描述符，仅关闭写方向；
                    // 失败无需处理（对端已关闭属正常情况）。
                    let _ = unsafe { libc::shutdown(output_fd, libc::SHUT_WR) };
                    return Ok(());
                }
                Ok(n) => break n,
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        poll_wait(input_fd, libc::POLLIN)?;
                        continue;
                    }
                    return Err(e);
                }
            }
        };

        let mut remaining = n;
        while remaining > 0 {
            match splice_once(pipe_reader, output_fd, remaining) {
                Ok(w) => remaining -= w,
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        poll_wait(output_fd, libc::POLLOUT)?;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
}