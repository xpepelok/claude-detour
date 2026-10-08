use crate::config::{Config, Protocol};
use crate::error::{ProxyError, Result};
use crate::{http, log, socks5, upstream};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

const ATTEMPTS: u32 = 5;
const TUNNEL_OK: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\n";

pub fn start_background(cfg: Arc<Config>) -> Result<bool> {
    match bind(&cfg) {
        Ok(listener) => {
            log!("listening on 127.0.0.1:{}", cfg.local_port);
            thread::spawn(move || accept_loop(listener, cfg));
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            log!(
                "port {} busy, assuming another instance forwards",
                cfg.local_port
            );
            Ok(false)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn run_foreground(cfg: Arc<Config>) -> Result<()> {
    let listener = bind(&cfg)?;
    log!("listening on 127.0.0.1:{}", cfg.local_port);
    accept_loop(listener, cfg);
    Ok(())
}

fn bind(cfg: &Config) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.local_port))
}

fn accept_loop(listener: TcpListener, cfg: Arc<Config>) {
    let gate = Arc::new(Semaphore::new(cfg.max_upstream.max(1)));
    for stream in listener.incoming() {
        let Ok(client) = stream else { continue };
        let cfg = Arc::clone(&cfg);
        let gate = Arc::clone(&gate);
        thread::spawn(move || {
            if let Err(e) = handle(client, &cfg, &gate) {
                log!("connection: {e}");
            }
        });
    }
}

fn handle(mut client: TcpStream, cfg: &Config, gate: &Semaphore) -> Result<()> {
    client.set_nodelay(true).ok();
    let head_bytes = http::read_head(&mut client)?;
    let head = http::parse_head(&head_bytes)?;
    let (host, port) = head.destination()?;

    let upstream = connect_upstream(cfg, &head, &host, port, gate)?;
    if head.is_connect() {
        client.write_all(TUNNEL_OK)?;
    }
    log!("{} {} -> open", head.method, head.target);
    splice(client, upstream);
    Ok(())
}

fn connect_upstream(
    cfg: &Config,
    head: &http::Head,
    host: &str,
    port: u16,
    gate: &Semaphore,
) -> Result<TcpStream> {
    let mut last = ProxyError::UpstreamClosed;
    for attempt in 1..=ATTEMPTS {
        let _permit = gate.acquire();
        match dial_and_negotiate(cfg, head, host, port) {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                log!("{} {} attempt {attempt}: {e}", head.method, head.target);
                last = e;
            }
        }
        drop(_permit);
        if attempt < ATTEMPTS {
            thread::sleep(Duration::from_millis(120 * attempt as u64));
        }
    }
    Err(last)
}

fn dial_and_negotiate(cfg: &Config, head: &http::Head, host: &str, port: u16) -> Result<TcpStream> {
    let mut stream = upstream::dial(cfg)?;
    let auth = cfg.credentials().map(|(u, p)| upstream::basic_auth(u, p));

    match (cfg.protocol, head.is_connect()) {
        (Protocol::Http, true) => {
            stream.write_all(&http::connect_head(host, port, auth.as_deref()))?;
            let reply = http::read_head(&mut stream)?;
            if !http::is_success_status(&reply) {
                return Err(ProxyError::HttpRejected(http::status_line(&reply)));
            }
        }
        (Protocol::Http, false) => {
            stream.write_all(&http::forward_head(head, auth.as_deref()))?;
        }
        (Protocol::Socks5, true) => {
            socks5::connect(&mut stream, host, port, cfg.credentials())?;
        }
        (Protocol::Socks5, false) => {
            socks5::connect(&mut stream, host, port, cfg.credentials())?;
            stream.write_all(&http::origin_form_head(head, host, port))?;
        }
    }
    Ok(stream)
}

fn splice(client: TcpStream, upstream: TcpStream) {
    let (c_read, c_write) = (client.try_clone(), client);
    let (u_read, u_write) = (upstream.try_clone(), upstream);
    let (Ok(c_read), Ok(u_read)) = (c_read, u_read) else {
        return;
    };
    let up = thread::spawn(move || pump(c_read, u_write));
    pump(u_read, c_write);
    let _ = up.join();
}

fn pump(mut from: TcpStream, mut to: TcpStream) {
    let mut buf = [0u8; 64 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = to.shutdown(std::net::Shutdown::Write);
}

struct Semaphore {
    slots: Mutex<usize>,
    cv: Condvar,
}

impl Semaphore {
    fn new(n: usize) -> Self {
        Semaphore {
            slots: Mutex::new(n),
            cv: Condvar::new(),
        }
    }

    fn acquire(&self) -> Permit<'_> {
        let mut slots = self.slots.lock().unwrap();
        while *slots == 0 {
            slots = self.cv.wait(slots).unwrap();
        }
        *slots -= 1;
        Permit { sem: self }
    }
}

struct Permit<'a> {
    sem: &'a Semaphore,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.sem.slots.lock().unwrap() += 1;
        self.sem.cv.notify_one();
    }
}
