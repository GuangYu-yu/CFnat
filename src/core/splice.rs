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
            libc::splice(
                src,
                std::ptr::null_mut(),
                dst,
                std::ptr::null_mut(),
                len,
                libc::SPLICE_F_MOVE,
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
        let n = match splice_once(input_fd, pipe_writer, BUF_SIZE) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) => return Err(e),
        };

        let mut remaining = n;
        while remaining > 0 {
            match splice_once(pipe_reader, output_fd, remaining) {
                Ok(w) => remaining -= w,
                Err(e) => return Err(e),
            }
        }
    }
}