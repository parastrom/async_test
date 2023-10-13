#[cfg(target_os = "linux")]
mod file;
#[cfg(target_os = "linux")]
mod socket;
#[cfg(target_os = "linux")]
mod uring_fut;
#[cfg(target_os = "linux")]
mod platform;

use std::time::Duration;
use std::mem;
use std::net::{SocketAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::io;

#[cfg(target_os = "linux")]
use io_uring::{opcode, types::Timespec};
#[cfg(target_os = "linux")]
pub use platform::*;
#[cfg(target_os = "linux")]
pub (crate) use  uring_fut::UringFut;
#[cfg(target_os = "linux")]
pub (crate) use file::*;
#[cfg(target_os = "linux")]
pub (crate) use socket::*;

type IoKey = u32;

const MAX_LIBC_SOCKADDR_SIZE: usize = mem::size_of::<libc::sockaddr_in6>();


pub async fn sleep(dur: Duration) {
    let timespec = Timespec::from(dur);
    let sqe = opcode::Timeout::new(&timespec).build();

    UringFut::new(sqe).await;
}

fn libc_addr_to_std(addr: &libc::sockaddr) -> SocketAddr {
    // IPv4 address
    if addr.sa_family == libc::AF_INET as libc::sa_family_t {
        // Reinterpret as sockaddr_in
        let addr = unsafe { &*(addr as *const libc::sockaddr as *const libc::sockaddr_in) };

        // Get params, converting from network to host endianness
        let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
        let port = u16::from_be(addr.sin_port);
        
        SocketAddr::V4(SocketAddrV4::new(ip, port))
    } else if addr.sa_family == libc::AF_INET6 as libc::sa_family_t { 
        // IPv6 address
        // Reinterpret as sockaddr_in6
        let addr = unsafe { &*(addr as *const libc::sockaddr as *const libc::sockaddr_in6) };

        let ip = Ipv6Addr::from(addr.sin6_addr.s6_addr);
        let port = u16::from_be(addr.sin6_port);
        let flowinfo = u32::from_be(addr.sin6_flowinfo);
        let scope_id = u32::from_be(addr.sin6_scope_id);

        SocketAddr::V6(SocketAddrV6::new(ip, port, flowinfo, scope_id))
    } else { // Unknown address family
        panic!("addr.sa_family has unexpected value `{}`", addr.sa_family);
    }
}


fn std_addr_to_libc(addr: &SocketAddr) -> [u8; MAX_LIBC_SOCKADDR_SIZE] {
    let mut buf = [0u8; MAX_LIBC_SOCKADDR_SIZE];

    match addr {
        // IPv4 address
        SocketAddr::V4(addr) => {
            // Interpret buf as sockaddr_in
            let out_addr = unsafe { &mut *(buf.as_mut_ptr() as *mut libc::sockaddr_in) };

            // Write address params, converting from host to network endianness
            out_addr.sin_family = libc::AF_INET as libc::sa_family_t;
            out_addr.sin_port = u16::to_be(addr.port());
            out_addr.sin_addr.s_addr = u32::to_be(u32::from(*addr.ip()));
        },

        // IPv6 address
        SocketAddr::V6(addr) => {
            // Interpret buf as sockaddr_in6
            let out_addr = unsafe { &mut *(buf.as_mut_ptr() as *mut libc::sockaddr_in6) };

            // Write address params, converting from host to network endianness
            out_addr.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            out_addr.sin6_port = u16::to_be(addr.port());
            out_addr.sin6_flowinfo = u32::to_be(addr.flowinfo());

            // These octets together are in host endianness
            // See the implementation of the Ipv6Addr::segments() fn for proof
            out_addr.sin6_addr.s6_addr = addr.ip().octets();

            out_addr.sin6_scope_id = u32::to_be(addr.scope_id());
        }
    }

    buf
}

fn libc_result_to_std(res: i32) -> io::Result<i32> {
    // Positive res means okay, negative means error and is equal
    // to the negated error code
    if res >= 0 {
        Ok(res)
    }
    else {
        Err(io::Error::from_raw_os_error(-res))
    }
}