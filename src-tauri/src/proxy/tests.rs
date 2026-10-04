use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn proxy_addresses_are_normalized_with_remote_dns_for_socks() {
    for (protocol, address, expected) in [
        (
            "http",
            "  Proxy.Example.com  ",
            "http://proxy.example.com:7890",
        ),
        ("https", "127.0.0.1", "https://127.0.0.1:7890"),
        ("socks5", "::1", "socks5h://[::1]:7890"),
        ("socks5", "[::1]", "socks5h://[::1]:7890"),
    ] {
        let settings = ProxySettings {
            protocol: protocol.into(),
            address: address.into(),
            port: 7890,
        };
        assert_eq!(settings.normalized().unwrap().url(), expected);
    }
}

fn http_proxy(
    response: &'static str,
    delay: Duration,
) -> (ProxySettings, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let settings = ProxySettings {
        protocol: "http".into(),
        address: "127.0.0.1".into(),
        port: listener.local_addr().unwrap().port(),
    };
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                Err(e) => panic!("proxy was not contacted: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        std::thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(request).unwrap()
    });
    (settings, server)
}

#[tokio::test]
async fn connection_test_sends_a_real_request_through_the_selected_proxy() {
    let (settings, server) = http_proxy(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        Duration::ZERO,
    );
    let result = probe(
        &settings,
        "http://unresolvable.invalid/robots.txt",
        Duration::from_secs(2),
    )
        .await
        .unwrap();
    assert_eq!(result.status, 200);
    assert!(server
        .join()
        .unwrap()
        .starts_with("GET http://unresolvable.invalid/robots.txt HTTP/1.1\r\n"));
}

#[tokio::test]
async fn connection_test_distinguishes_target_rejection_authentication_and_timeout() {
    for (response, target, delay, code) in [
        (
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n",
            "http://unresolvable.invalid/",
            Duration::ZERO,
            "targetHttpError",
        ),
        (
            "HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
            "https://unresolvable.invalid/",
            Duration::ZERO,
            "proxyAuthRequired",
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
            "http://unresolvable.invalid/",
            Duration::from_millis(300),
            "timeout",
        ),
    ] {
        let (settings, server) = http_proxy(response, delay);
        let result = probe(&settings, target, Duration::from_millis(150))
            .await
            .unwrap_err();
        assert_eq!(result.code, code);
        server.join().unwrap();
    }
}

#[tokio::test]
async fn connection_test_does_not_accept_an_open_port_or_fallback_to_direct() {
    let (settings, server) = http_proxy("not a proxy\r\n\r\n", Duration::ZERO);
    assert!(probe(
        &settings,
        "https://unresolvable.invalid/",
        Duration::from_secs(2)
    )
        .await
        .is_err());
    assert!(server
        .join()
        .unwrap()
        .starts_with("CONNECT unresolvable.invalid:443 HTTP/1.1"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let settings = ProxySettings { port, ..settings };
    // Windows may take over two seconds to report a refused loopback connection.
    assert_eq!(
        probe(
            &settings,
            "https://unresolvable.invalid/",
            Duration::from_secs(5)
        )
            .await
            .unwrap_err()
            .code,
        "connectionRefused"
    );
}

#[tokio::test]
async fn socks_test_uses_proxy_dns_and_reports_authentication_rejection() {
    for authentication_required in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = ProxySettings {
            protocol: "socks5".into(),
            address: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
        };
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                    Err(e) => panic!("SOCKS proxy was not contacted: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            stream
                .write_all(&[5, if authentication_required { 255 } else { 0 }])
                .unwrap();
            if authentication_required {
                return;
            }
            let mut header = [0; 5];
            stream.read_exact(&mut header).unwrap();
            assert_eq!(&header[..4], &[5, 1, 0, 3]);
            let mut host = vec![0; header[4] as usize];
            stream.read_exact(&mut host).unwrap();
            assert_eq!(host, b"unresolvable.invalid");
            let mut port = [0; 2];
            stream.read_exact(&mut port).unwrap();
            assert_eq!(u16::from_be_bytes(port), 80);
            stream
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(request.starts_with(b"GET /robots.txt HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .unwrap();
        });
        let result = probe(
            &settings,
            "http://unresolvable.invalid/robots.txt",
            Duration::from_secs(2),
        )
            .await;
        server.join().unwrap();
        if authentication_required {
            assert_eq!(result.unwrap_err().code, "proxyAuthRequired");
        } else {
            assert_eq!(result.unwrap().status, 200);
        }
    }
}

#[test]
fn proxy_validation_rejects_urls_credentials_paths_and_invalid_ports() {
    for address in [
        "",
        "http://127.0.0.1",
        "proxy.example:7890",
        "user@proxy.example",
        "proxy/path",
        "proxy?x",
        "proxy#x",
        "host name",
        "-bad.example",
        "host%20name",
    ] {
        let settings = ProxySettings {
            protocol: "http".into(),
            address: address.into(),
            port: 7890,
        };
        assert_eq!(
            settings.normalized().unwrap_err().code,
            "invalidSettings",
            "{address}"
        );
    }
    for (protocol, port) in [("ftp", 7890), ("http", 0)] {
        assert!(ProxySettings {
            protocol: protocol.into(),
            address: "127.0.0.1".into(),
            port
        }
            .normalized()
            .is_err());
    }
}
