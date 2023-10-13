use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use crate::fs::OpenOptions;
use super::uring_fut::UringFut;
use super::libc_result_to_std;
use io_uring::opcode;
use io_uring::types::Fd;
use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd};
use crate::RUNTIME;

pub async fn file_open(path: &Path, opts: &OpenOptions) -> io::Result<File> {
    let mut flags = match (opts.read, opts.write) {
        (true, false) => libc::O_RDONLY,
        (false, true) => libc::O_WRONLY,
        (true, true) => libc::O_RDWR,
        (false, false) => 0
    };

    if opts.append {
        flags |= libc::O_APPEND;
    }

    if opts.truncate {
        flags |= libc::O_TRUNC;
    }

    if opts.create || opts.create_new {
        flags |= libc::O_CREAT;
    }

    if opts.create_new {
        flags |= libc::O_EXCL;
    }

    let dirfd = Fd(libc::AT_FDCWD);
    let path = CString::new(path.as_os_str().as_bytes()).expect("Path contained null byte");

    let sqe = opcode::OpenAt::new(dirfd, path.as_ptr() as *const _)
        .flags(flags)
        .mode(0o666)
        .build();

    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|fd| unsafe { File::from_raw_fd(fd) })
}

pub async fn file_read(file: &File, buf: &mut [u8]) -> io::Result<usize> {
    let sqe = opcode::Read::new(Fd(file.as_raw_fd()), buf.as_mut_ptr(), buf.len() as u32).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|bytes| bytes as usize)
}

pub async fn file_write(file: &File, buf: &[u8]) -> io::Result<usize> {
    let sqe = opcode::Write::new(Fd(file.as_raw_fd()), buf.as_ptr(), buf.len() as u32).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|bytes| bytes as usize)
}

pub fn file_close(file: &File) {
    RUNTIME.with_borrow_mut(|rt| {
        let sqe = opcode::Close::new(Fd(file.as_raw_fd()))
            .build()
            .user_data(0); // IoKey 0 reserved for fd closes

        rt.plat.submit_sqe(sqe);   
    });
}
