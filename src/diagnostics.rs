use crate::config::{Config, Protocol};
use crate::error::{ProxyError, Result};
use crate::{http, socks5, upstream};
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener};

const PROBE_HOSTS: [&str; 3] = ["claude.ai", "api.anthropic.com", "example.com"];

pub fn run(cfg: &Config) -> bool {
    println!(
        "Proxy: {}://{}:{}   login: {}",
        cfg.protocol,
        cfg.host,
        cfg.port,
        cfg.user.as_deref().unwrap_or("(none)")
    );
    println!("Local port: {}\n", cfg.local_port);

    let mut ok = true;

    match TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.local_port)) {
        Ok(_) => println!("[OK]   local port 127.0.0.1:{} is free", cfg.local_port),
        Err(_) => println!(
            "[INFO] local port {} is busy; it will be reused or freed on launch",
            cfg.local_port
        ),
    }

    match upstream::dial(cfg) {
        Ok(_) => println!("[OK]   TCP connection to the proxy established"),
        Err(e) => {
            ok = false;
            println!("[FAIL] no TCP to the proxy: {e}");
            report(ok);
            return ok;
        }
    }

    for host in PROBE_HOSTS {
        match probe(cfg, host, 443) {
            Ok(()) => println!("[OK]   CONNECT {host} -> tunnel established"),
            Err(e) => {
                ok = false;
                println!("[FAIL] CONNECT {host}: {e}");
            }
        }
    }

    report(ok);
    ok
}

fn probe(cfg: &Config, host: &str, port: u16) -> Result<()> {
    let mut stream = upstream::dial(cfg)?;
    match cfg.protocol {
        Protocol::Http => {
            let auth = cfg.credentials().map(|(u, p)| upstream::basic_auth(u, p));
            stream.write_all(&http::connect_head(host, port, auth.as_deref()))?;
            let reply = http::read_head(&mut stream)?;
            if !http::is_success_status(&reply) {
                return Err(ProxyError::HttpRejected(http::status_line(&reply)));
            }
            Ok(())
        }
        Protocol::Socks5 => socks5::connect(&mut stream, host, port, cfg.credentials()),
    }
}

fn report(ok: bool) {
    println!(
        "\n{}",
        if ok {
            "RESULT: OK, the proxy works"
        } else {
            "RESULT: FAILED, see the [FAIL] lines above"
        }
    );
}
