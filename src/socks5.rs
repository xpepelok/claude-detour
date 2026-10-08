use crate::error::{ProxyError, Result};
use std::io::{Read, Write};

const VERSION: u8 = 0x05;
const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;
const METHOD_NONE: u8 = 0x00;
const METHOD_USERPASS: u8 = 0x02;
const METHOD_REJECT: u8 = 0xFF;

pub fn greeting(has_credentials: bool) -> Vec<u8> {
    if has_credentials {
        vec![VERSION, 2, METHOD_NONE, METHOD_USERPASS]
    } else {
        vec![VERSION, 1, METHOD_NONE]
    }
}

pub fn userpass(user: &str, pass: &str) -> Vec<u8> {
    let mut out = vec![0x01, user.len() as u8];
    out.extend_from_slice(user.as_bytes());
    out.push(pass.len() as u8);
    out.extend_from_slice(pass.as_bytes());
    out
}

pub fn connect_request(host: &str, port: u16) -> Result<Vec<u8>> {
    if host.len() > 255 {
        return Err(ProxyError::Socks5("host name longer than 255 bytes".into()));
    }
    let mut out = vec![VERSION, CMD_CONNECT, 0x00, ATYP_DOMAIN, host.len() as u8];
    out.extend_from_slice(host.as_bytes());
    out.extend_from_slice(&port.to_be_bytes());
    Ok(out)
}

pub fn connect<S: Read + Write>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<(&str, &str)>,
) -> Result<()> {
    stream.write_all(&greeting(credentials.is_some()))?;

    let mut method = [0u8; 2];
    stream.read_exact(&mut method)?;
    if method[0] != VERSION {
        return Err(ProxyError::Socks5(format!(
            "unexpected version {:#x} in method reply",
            method[0]
        )));
    }
    match method[1] {
        METHOD_NONE => {}
        METHOD_USERPASS => {
            let (user, pass) = credentials.ok_or_else(|| {
                ProxyError::Socks5("proxy requires a login we do not have".into())
            })?;
            stream.write_all(&userpass(user, pass))?;
            let mut status = [0u8; 2];
            stream.read_exact(&mut status)?;
            if status[1] != 0x00 {
                return Err(ProxyError::Socks5("login rejected".into()));
            }
        }
        METHOD_REJECT => {
            return Err(ProxyError::Socks5(
                "proxy rejected every authentication method".into(),
            ))
        }
        other => {
            return Err(ProxyError::Socks5(format!(
                "proxy selected unsupported method {other:#x}"
            )))
        }
    }

    stream.write_all(&connect_request(host, port)?)?;
    read_connect_reply(stream)
}

pub fn read_connect_reply<S: Read>(stream: &mut S) -> Result<()> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    if head[0] != VERSION {
        return Err(ProxyError::Socks5(format!(
            "unexpected version {:#x} in connect reply",
            head[0]
        )));
    }
    if head[1] != 0x00 {
        return Err(ProxyError::Socks5(reply_reason(head[1]).to_string()));
    }
    let tail = match head[3] {
        ATYP_IPV4 => 4 + 2,
        ATYP_IPV6 => 16 + 2,
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            len[0] as usize + 2
        }
        other => {
            return Err(ProxyError::Socks5(format!(
                "unknown address type {other:#x}"
            )))
        }
    };
    let mut discard = vec![0u8; tail];
    stream.read_exact(&mut discard)?;
    Ok(())
}

fn reply_reason(code: u8) -> &'static str {
    match code {
        0x01 => "general SOCKS server failure",
        0x02 => "connection not allowed by ruleset",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "connection rejected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn greeting_depends_on_credentials() {
        assert_eq!(greeting(false), vec![0x05, 1, 0x00]);
        assert_eq!(greeting(true), vec![0x05, 2, 0x00, 0x02]);
    }

    #[test]
    fn userpass_layout() {
        assert_eq!(
            userpass("ab", "xyz"),
            vec![0x01, 2, b'a', b'b', 3, b'x', b'y', b'z']
        );
    }

    #[test]
    fn connect_request_domain_and_port() {
        let req = connect_request("claude.ai", 443).unwrap();
        assert_eq!(&req[..5], &[0x05, 0x01, 0x00, 0x03, 9]);
        assert_eq!(&req[5..14], b"claude.ai");
        assert_eq!(&req[14..], &[0x01, 0xBB]);
    }

    #[test]
    fn connect_request_rejects_long_host() {
        let host = "a".repeat(256);
        assert!(connect_request(&host, 443).is_err());
    }

    #[test]
    fn reply_ok_consumes_ipv4_tail() {
        let mut c = Cursor::new(vec![0x05, 0x00, 0x00, 0x01, 1, 2, 3, 4, 0x00, 0x50]);
        read_connect_reply(&mut c).unwrap();
        assert_eq!(c.position() as usize, 10);
    }

    #[test]
    fn reply_ok_consumes_domain_tail() {
        let mut c = Cursor::new(vec![
            0x05, 0x00, 0x00, 0x03, 3, b'a', b'b', b'c', 0x01, 0xBB,
        ]);
        read_connect_reply(&mut c).unwrap();
        assert_eq!(c.position() as usize, 10);
    }

    #[test]
    fn reply_failure_is_reported() {
        let mut c = Cursor::new(vec![0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        let err = read_connect_reply(&mut c).unwrap_err();
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn full_handshake_with_login() {
        let server = [
            0x05, 0x02, 0x01, 0x00, 0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0,
        ];
        let mut stream = Duplex::new(server.to_vec());
        connect(&mut stream, "claude.ai", 443, Some(("u", "p"))).unwrap();
        let w = stream.written;
        assert_eq!(&w[..4], &[0x05, 0x02, 0x00, 0x02]);
        assert_eq!(&w[4..9], &[0x01, 1, b'u', 1, b'p']);
        assert_eq!(w[9], 0x05);
    }

    struct Duplex {
        to_read: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl Duplex {
        fn new(to_read: Vec<u8>) -> Self {
            Duplex {
                to_read: Cursor::new(to_read),
                written: Vec::new(),
            }
        }
    }

    impl Read for Duplex {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.to_read.read(buf)
        }
    }

    impl Write for Duplex {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
