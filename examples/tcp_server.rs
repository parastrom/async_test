use std::net::SocketAddr;

use uring_test::net::TcpListener;

pub fn main() {
    uring_test::init().unwrap();

    uring_test::run(async {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 8080))).await.unwrap();

        loop {
            let (stream, addr) = listener.accept().await.unwrap();

            println!("Accepted connection from {:?}", addr);

            uring_test::spawn(async move {
                let mut buf = [0u8; 1024];

                loop {
                    let n = stream.read(&mut buf).await.unwrap();

                    if n == 0 {
                        println!("Connection closed by {:?}", addr);
                        break;
                    }

                    stream.write(&buf[..n]).await.unwrap();
                }
            });
        }
    });
}