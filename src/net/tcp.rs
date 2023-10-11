use std::io::Result;
use std::net::{SocketAddr, ToSocketAddrs, Shutdown};
use std::mem::ManuallyDrop;

use crate::{
    util::try_zip,
    platform::{
        socket_create,
        socket_close,
        socket_connect,
        socket_recv,
        socket_send,
        socket_accept,
        socket_shutdown,
    }
};

pub struct TcpStream(ManuallyDrop<std::net::TcpStream>);

impl TcpStream {
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let addr_iter = addr.to_socket_addrs().expect("Couldn't get address iterator");

        let (stream_v4, stream_v6) = try_zip(
            socket_create::<std::net::TcpStream>(false, false), 
            socket_create::<std::net::TcpStream>(true, false)
        ).await?;

        // Prevent the streams from being auto dropped since we need to manually drop them
        let stream_v4 = ManuallyDrop::new(stream_v4);
        let stream_v6 = ManuallyDrop::new(stream_v6);

        let mut res = None;

        for addr in addr_iter {
            match addr {
                SocketAddr::V4(_) => {
                    match socket_connect(&*stream_v4, &addr).await {
                        Ok(()) => {
                            socket_close(&*stream_v6);
                            stream_v4.set_nonblocking(true)?;
                            return Ok(Self(stream_v4));
                        },

                        Err(err) => res = Some(err)
                    }
                },

                SocketAddr::V6(_) => {
                    match socket_connect(&*stream_v6, &addr).await {
                        Ok(()) => {
                            socket_close(&*stream_v4);
                            stream_v6.set_nonblocking(true)?;
                            return Ok(Self(stream_v6));
                        },

                        Err(err) => res = Some(err)
                    }
                }
            }
        }

        match res {
            Some(err) => Err(err),
            None => panic!("Address iterator didn't provide any addresses")
        }
    }

    pub fn std(&self) -> &std::net::TcpStream {
        &self.0
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        socket_recv(&*self.0, buf, false).await
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize> {
        socket_send(&*self.0, buf).await
    }

    pub async fn shutdown(&self, how: Shutdown) -> Result<()> {
        socket_shutdown(&*self.0, how).await
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        socket_close(&*self.0);
    }
}

pub struct TcpListener(ManuallyDrop<std::net::TcpListener>);

impl TcpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;

        Ok(Self(ManuallyDrop::new(listener)))
    }

    pub fn std(&self) -> &std::net::TcpListener {
        &self.0
    }

    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr)> {
        let res = socket_accept(&*self.0).await;

        // Map from std TcpStream to our own TcpStream type
        res.map(|(stream, addr)| (TcpStream(ManuallyDrop::new(stream)), addr))
    }
} 

impl Drop for TcpListener {
    fn drop(&mut self) {
        socket_close(&*self.0);
    }
} 