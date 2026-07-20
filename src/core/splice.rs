use std::io;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};

const BUF_SIZE: usize = 64 * 1024;
const IDLE_TIMEOUT_MS: i32 = 60_000;

struct Pipe {
    reader: OwnedFd,
    writer: OwnedFd,
}

impl Pipe {
    fn new() -> io::Result<Self> {
        let mut fds = [-1i32; 2];
        // SAFETY: pipe2 调用使用正确大小的缓冲区(fds)和标准标志位。
        // O_CLOEXEC 和 O_NONBLOCK 是标准标志，返回值已检查错误。
        let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
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

fn wait_for_event(fd: RawFd, events: i16) -> io::Result<()> {
    let mut poll_fd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    // SAFETY: poll_fd 是正确初始化的 pollfd 结构体，包含有效的 fd 和
    // 请求的事件。超时时间为正数。即使 fd 无效，poll 也是安全的
    //（错误通过 revents 返回）。
    let ret = unsafe { libc::poll(&mut poll_fd, 1, IDLE_TIMEOUT_MS) };
    match ret {
        n if n < 0 => {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                Ok(())
            } else {
                Err(err)
            }
        }
        0 => Err(io::Error::new(io::ErrorKind::TimedOut, "splice idle timeout")),
        _ => {
            if poll_fd.revents & events != 0 {
                Ok(())
            } else if poll_fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "fd error/hup",
                ))
            } else {
                Ok(())
            }
        }
    }
}

fn splice_once(src: RawFd, dst: RawFd, len: usize) -> io::Result<usize> {
    loop {
        // SAFETY: src 和 dst 是本进程拥有的有效文件描述符。
        // 偏移量传空指针适用于管道/套接字（没有文件偏移概念）。
        // SPLICE_F_MOVE 和 SPLICE_F_NONBLOCK 是安全的标志组合。
        let n = unsafe {
            libc::splice(
                src,
                std::ptr::null_mut(),
                dst,
                std::ptr::null_mut(),
                len,
                libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK,
            )
        };
        if n >= 0 {
            return Ok(n as usize);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn splice_loop(
    input_fd: RawFd,
    output_fd: RawFd,
    pipe_reader: RawFd,
    pipe_writer: RawFd,
) -> io::Result<()> {
    loop {
        wait_for_event(input_fd, libc::POLLIN)?;

        let n = match splice_once(input_fd, pipe_writer, BUF_SIZE) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        };

        let mut remaining = n;
        while remaining > 0 {
            wait_for_event(output_fd, libc::POLLOUT)?;

            match splice_once(pipe_reader, output_fd, remaining) {
                Ok(w) => remaining -= w,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        }
    }
}