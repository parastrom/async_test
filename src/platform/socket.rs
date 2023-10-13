use std::io;
use std::net::{TcpStream, SocketAddr, Shutdown};
use std::os::fd::{FromRawFd, AsRawFd};
use io_uring::opcode;
use io_uring::types::Fd;
use crate::RUNTIME;

use super::{libc_result_to_std, std_addr_to_libc, MAX_LIBC_SOCKADDR_SIZE, libc_addr_to_std};
use super::uring_fut::UringFut;

pub async fn socket_create<T: FromRawFd>(ipv6: bool, udp: bool) -> io::Result<T> {
    let domain = if ipv6 { libc::AF_INET6 } else { libc::AF_INET };
    let socket_type = if udp { libc::SOCK_DGRAM } else { libc::SOCK_STREAM };
    let protocol = if udp { libc::IPPROTO_UDP } else { libc::IPPROTO_TCP };

    let sqe = opcode::Socket::new(domain, socket_type, protocol).build();
    let res = UringFut::new(sqe).await;

    let fd = libc_result_to_std(res);

    fd.map(|fd| unsafe { T::from_raw_fd(fd) })
}

pub fn socket_close<T: AsRawFd>(sock: &T) {
    RUNTIME.with_borrow_mut(|rt| {
        let sqe = opcode::Close::new(Fd(sock.as_raw_fd()))
            .build()
            .user_data(0); // IoKey 0 reserved for fd closes

        rt.plat.submit_sqe(sqe);   
    });
}

pub async fn socket_connect<T: AsRawFd>(sock: &T, addr: &SocketAddr) -> io::Result<()> {
    let addr = std_addr_to_libc(addr);

    let sqe = opcode::Connect::new(Fd(sock.as_raw_fd()), addr.as_ptr() as *const libc::sockaddr, addr.len() as u32).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|_| ())
}

pub async fn socket_recv<T: AsRawFd>(sock: &T, buf: &mut [u8], peek: bool) -> io::Result<usize> {
    let sqe = opcode::Recv::new(Fd(sock.as_raw_fd()), buf.as_mut_ptr(), buf.len() as u32)
        .flags(if peek { libc::MSG_PEEK } else { 0 })
        .build();

    let res = UringFut::new(sqe).await;
    libc_result_to_std(res).map(|bytes| bytes as usize)
}

pub async fn socket_recv_from<T: AsRawFd>(sock: &T, buf: &mut [u8], peek: bool) -> io::Result<(usize, SocketAddr)> {
    // Since a future is always pinned before use, these variable will have
    // a stable address that we can pass to the kernel without boxing
    // This approach saves us a heap allocation
    let mut iovec = libc::iovec {
        iov_base: buf.as_ptr() as *mut _,
        iov_len: buf.len()
    };

    // Create buffer with sufficient space to hold the largest sockaddr that we're expecting
    let mut src_addr = [0u8; MAX_LIBC_SOCKADDR_SIZE];

    let mut msghdr = libc::msghdr {
        msg_name: src_addr.as_mut_ptr() as *mut _,
        msg_namelen: src_addr.len() as u32,
        msg_iov: &mut iovec,
        msg_iovlen: 1,
        msg_control: std::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: if peek { libc::MSG_PEEK } else { 0 }
    };

    let sqe = opcode::RecvMsg::new(Fd(sock.as_raw_fd()), &mut msghdr).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|bytes| {
        let src_addr = unsafe { &*(src_addr.as_ptr() as *const _) };
        let src_addr = libc_addr_to_std(src_addr);

        (bytes as usize, src_addr)
    })
}

pub async fn socket_send<T: AsRawFd>(sock: &T, buf: &[u8]) -> io::Result<usize> {
    let sqe = opcode::Send::new(Fd(sock.as_raw_fd()), buf.as_ptr(), buf.len() as u32).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|bytes| bytes as usize)
}

pub async fn socket_send_to<T: AsRawFd>(sock: &T, buf: &[u8], addr: &SocketAddr) -> io::Result<usize> {
    // A future is always pinned before use, so these will have a static address,
    // that we can pass to the kernel without boxing.
    //
    // Since  these are self referential, we can't use a move closure, so we
    // use a move async block instead to capture the variables by value and
    // move them into the async block's state.
    let mut iovec = libc::iovec {
        iov_base: buf.as_ptr() as *mut _,
        iov_len: buf.len()
    };

    let mut addr = std_addr_to_libc(&addr);

    let mut msghdr = libc::msghdr {
        msg_name: addr.as_mut_ptr() as *mut _,
        msg_namelen: addr.len() as u32,
        msg_iov: &mut iovec,
        msg_iovlen: 1,
        msg_control: std::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: 0
    };

    let sqe = opcode::SendMsg::new(Fd(sock.as_raw_fd()), &mut msghdr).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|bytes| bytes as usize)
}

pub async fn socket_accept<T: AsRawFd>(sock: &T) -> io::Result<(TcpStream, SocketAddr)> {
    // Create buffer with sufficient space to hold the largest sockaddr that we're expecting
    let mut sockaddr = [0u8; MAX_LIBC_SOCKADDR_SIZE];
    let mut addrlen = MAX_LIBC_SOCKADDR_SIZE as libc::socklen_t;

    let libc_addr = sockaddr.as_mut_ptr() as *mut libc::sockaddr;

    let sqe = opcode::Accept::new(Fd(sock.as_raw_fd()), libc_addr, &mut addrlen).build();
    let res = UringFut::new(sqe).await;

    let fd = libc_result_to_std(res);

    fd.map(|fd| {
        let stream = unsafe { TcpStream::from_raw_fd(fd) };

        let peer_addr = unsafe { &*libc_addr };
        let peer_addr = libc_addr_to_std(peer_addr);

        (stream, peer_addr)
    })
}

pub async fn socket_shutdown<T: AsRawFd>(sock: &T, how: Shutdown) -> io::Result<()> {
    let how = match how {
        Shutdown::Read => libc::SHUT_RD,
        Shutdown::Write => libc::SHUT_WR,
        Shutdown::Both => libc::SHUT_RDWR
    };

    let sqe = opcode::Shutdown::new(Fd(sock.as_raw_fd()), how).build();
    let res = UringFut::new(sqe).await;

    libc_result_to_std(res).map(|_| ())
}
