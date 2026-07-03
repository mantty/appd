use std::io::Write;
use std::net::{TcpListener, TcpStream};

use appd_runtime::{LocalBackend, platform_socket_id};

#[test]
fn binds_loopback_on_dynamic_port_for_workerd_listener_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = LocalBackend::bind_loopback()?;
    let port = backend.local_port();
    let listener = backend.into_listener();

    assert_eq!(listener.local_addr()?.port(), port);

    let client = std::thread::spawn(move || -> std::io::Result<()> {
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.write_all(b"GET / HTTP/1.1\r\n\r\n")?;
        Ok(())
    });

    let (server, _) = listener.accept()?;
    client.join().map_err(|_| "client thread panicked")??;

    assert_ne!(platform_socket_id(&server), 0);
    Ok(())
}

#[test]
fn exposes_owned_listener_without_rebinding_port() -> Result<(), Box<dyn std::error::Error>> {
    let backend = LocalBackend::bind_loopback()?;
    let port = backend.local_port();
    let listener = backend.into_listener();

    let rebound = TcpListener::bind(("127.0.0.1", port));

    assert!(rebound.is_err());
    drop(listener);
    Ok(())
}
