use claude_detour::config::{Config, Protocol};
use claude_detour::proxy;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(protocol: Protocol, upstream_port: u16, creds: Option<(&str, &str)>) -> Arc<Config> {
    Arc::new(Config {
        protocol,
        host: "127.0.0.1".into(),
        port: upstream_port,
        user: creds.map(|(u, _)| u.to_string()),
        pass: creds.map(|(_, p)| p.to_string()),
        local_port: free_port(),
        max_upstream: 4,
    })
}

fn connect_retry(port: u16) -> TcpStream {
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
            return s;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("local proxy never came up on {port}");
}

fn read_head(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut one = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut one).unwrap();
        assert!(n > 0, "connection closed before head finished");
        buf.push(one[0]);
    }
    buf
}

fn echo(mut stream: TcpStream) {
    let mut buf = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 || stream.write_all(&buf[..n]).is_err() {
            break;
        }
    }
}

fn spawn_http_proxy(expect_auth: Option<String>) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let expect_auth = expect_auth.clone();
            let mut stream = stream.unwrap();
            thread::spawn(move || {
                let head = String::from_utf8(read_head(&mut stream)).unwrap();
                if let Some(auth) = expect_auth {
                    let line = format!("Proxy-Authorization: Basic {auth}\r\n");
                    if !head.contains(&line) {
                        stream
                            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                            .unwrap();
                        return;
                    }
                }
                stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
                echo(stream);
            });
        }
    });
    port
}

fn spawn_socks5_proxy(expect_auth: Option<(String, String)>) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let expect_auth = expect_auth.clone();
            let mut s = stream.unwrap();
            thread::spawn(move || {
                let mut g = [0u8; 2];
                s.read_exact(&mut g).unwrap();
                let mut methods = vec![0u8; g[1] as usize];
                s.read_exact(&mut methods).unwrap();

                if let Some((user, pass)) = expect_auth {
                    assert!(methods.contains(&0x02), "client did not offer user/pass");
                    s.write_all(&[0x05, 0x02]).unwrap();
                    let mut vh = [0u8; 2];
                    s.read_exact(&mut vh).unwrap();
                    let mut u = vec![0u8; vh[1] as usize];
                    s.read_exact(&mut u).unwrap();
                    let mut pl = [0u8; 1];
                    s.read_exact(&mut pl).unwrap();
                    let mut p = vec![0u8; pl[0] as usize];
                    s.read_exact(&mut p).unwrap();
                    let good = u == user.as_bytes() && p == pass.as_bytes();
                    s.write_all(&[0x01, if good { 0x00 } else { 0x01 }])
                        .unwrap();
                    if !good {
                        return;
                    }
                } else {
                    s.write_all(&[0x05, 0x00]).unwrap();
                }

                let mut h = [0u8; 4];
                s.read_exact(&mut h).unwrap();
                let alen = match h[3] {
                    0x01 => 4,
                    0x04 => 16,
                    0x03 => {
                        let mut b = [0u8; 1];
                        s.read_exact(&mut b).unwrap();
                        b[0] as usize
                    }
                    _ => 0,
                };
                let mut tail = vec![0u8; alen + 2];
                s.read_exact(&mut tail).unwrap();
                s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .unwrap();
                echo(s);
            });
        }
    });
    port
}

fn tunnel_and_echo(cfg: Arc<Config>) {
    let local = cfg.local_port;
    proxy::start_background(cfg).unwrap();
    let mut client = connect_retry(local);
    client
        .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
        .unwrap();
    let reply = read_head(&mut client);
    assert!(
        reply.starts_with(b"HTTP/1.1 200"),
        "got: {}",
        String::from_utf8_lossy(&reply)
    );
    client.write_all(b"ping through the tunnel").unwrap();
    let mut got = [0u8; 23];
    client.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"ping through the tunnel");
}

#[test]
fn http_upstream_tunnels() {
    let port = spawn_http_proxy(None);
    tunnel_and_echo(config(Protocol::Http, port, None));
}

#[test]
fn http_upstream_passes_credentials() {
    let port = spawn_http_proxy(Some("dXNlcjpwYXNz".into()));
    tunnel_and_echo(config(Protocol::Http, port, Some(("user", "pass"))));
}

#[test]
fn socks5_upstream_tunnels() {
    let port = spawn_socks5_proxy(None);
    tunnel_and_echo(config(Protocol::Socks5, port, None));
}

#[test]
fn socks5_upstream_passes_credentials() {
    let port = spawn_socks5_proxy(Some(("alice".into(), "s3cret".into())));
    tunnel_and_echo(config(Protocol::Socks5, port, Some(("alice", "s3cret"))));
}
