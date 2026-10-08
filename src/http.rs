use crate::error::{ProxyError, Result};
use std::io::Read;

const MAX_HEAD: usize = 64 * 1024;

pub struct Head {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn is_connect(&self) -> bool {
        self.method.eq_ignore_ascii_case("CONNECT")
    }

    pub fn destination(&self) -> Result<(String, u16)> {
        if self.is_connect() {
            return split_host_port(&self.target, 443);
        }
        let rest = self
            .target
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or(&self.target);
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        split_host_port(authority, 80)
    }

    pub fn origin_path(&self) -> String {
        if let Some((_, rest)) = self.target.split_once("://") {
            match rest.find(['/']) {
                Some(i) => rest[i..].to_string(),
                None => "/".to_string(),
            }
        } else if self.target.starts_with('/') {
            self.target.clone()
        } else {
            "/".to_string()
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub fn read_head<R: Read>(stream: &mut R) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(1024);
    let mut one = [0u8; 1];
    while buf.len() < MAX_HEAD {
        let n = stream.read(&mut one)?;
        if n == 0 {
            return Err(ProxyError::UpstreamClosed);
        }
        buf.push(one[0]);
        if buf.ends_with(b"\r\n\r\n") {
            return Ok(buf);
        }
    }
    Err(ProxyError::BadRequest)
}

pub fn parse_head(bytes: &[u8]) -> Result<Head> {
    let text = std::str::from_utf8(bytes).map_err(|_| ProxyError::BadRequest)?;
    let mut lines = text.split("\r\n");
    let first = lines.next().ok_or(ProxyError::BadRequest)?;
    let mut parts = first.split(' ');
    let method = parts.next().ok_or(ProxyError::BadRequest)?.to_string();
    let target = parts.next().ok_or(ProxyError::BadRequest)?.to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    if method.is_empty() || target.is_empty() {
        return Err(ProxyError::BadRequest);
    }

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(Head {
        method,
        target,
        version,
        headers,
    })
}

pub fn connect_head(host: &str, port: u16, auth: Option<&str>) -> Vec<u8> {
    let target = format!("{host}:{port}");
    let mut out = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(credentials) = auth {
        out.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

pub fn forward_head(head: &Head, auth: Option<&str>) -> Vec<u8> {
    let mut out = format!("{} {} {}\r\n", head.method, head.target, head.version);
    append_relayed_headers(&mut out, head);
    if let Some(credentials) = auth {
        out.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

pub fn origin_form_head(head: &Head, host: &str, port: u16) -> Vec<u8> {
    let mut out = format!(
        "{} {} {}\r\n",
        head.method,
        head.origin_path(),
        head.version
    );
    if head.header("host").is_none() {
        let host_value = if port == 80 {
            host.to_string()
        } else {
            format!("{host}:{port}")
        };
        out.push_str(&format!("Host: {host_value}\r\n"));
    }
    append_relayed_headers(&mut out, head);
    out.push_str("\r\n");
    out.into_bytes()
}

fn append_relayed_headers(out: &mut String, head: &Head) {
    for (k, v) in &head.headers {
        if k.eq_ignore_ascii_case("Proxy-Authorization")
            || k.eq_ignore_ascii_case("Proxy-Connection")
        {
            continue;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
}

pub fn is_success_status(reply: &[u8]) -> bool {
    status_line(reply)
        .split(' ')
        .nth(1)
        .map(|code| code.starts_with('2'))
        .unwrap_or(false)
}

pub fn status_line(reply: &[u8]) -> String {
    let text = String::from_utf8_lossy(reply);
    text.split("\r\n").next().unwrap_or("").to_string()
}

fn split_host_port(authority: &str, default_port: u16) -> Result<(String, u16)> {
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port = port
                .parse::<u16>()
                .map_err(|_| ProxyError::BadTarget(authority.to_string()))?;
            Ok((host.to_string(), port))
        }
        _ if !authority.is_empty() => Ok((authority.to_string(), default_port)),
        _ => Err(ProxyError::BadTarget(authority.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &str) -> Head {
        parse_head(raw.as_bytes()).unwrap()
    }

    #[test]
    fn parses_connect() {
        let h = head("CONNECT claude.ai:443 HTTP/1.1\r\nHost: claude.ai:443\r\n\r\n");
        assert!(h.is_connect());
        assert_eq!(h.destination().unwrap(), ("claude.ai".to_string(), 443));
    }

    #[test]
    fn parses_absolute_get() {
        let h = head("GET http://example.com/a?b=1 HTTP/1.1\r\nHost: example.com\r\n\r\n");
        assert!(!h.is_connect());
        assert_eq!(h.destination().unwrap(), ("example.com".to_string(), 80));
        assert_eq!(h.origin_path(), "/a?b=1");
    }

    #[test]
    fn absolute_uri_without_path_is_root() {
        let h = head("GET http://example.com HTTP/1.1\r\nHost: example.com\r\n\r\n");
        assert_eq!(h.destination().unwrap(), ("example.com".to_string(), 80));
        assert_eq!(h.origin_path(), "/");
    }

    #[test]
    fn connect_head_includes_auth() {
        let bytes = connect_head("claude.ai", 443, Some("dXNlcjpwYXNz"));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("CONNECT claude.ai:443 HTTP/1.1\r\n"));
        assert!(text.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn forward_head_strips_proxy_headers() {
        let h = head(
            "GET http://x/ HTTP/1.1\r\nHost: x\r\nProxy-Connection: keep-alive\r\nProxy-Authorization: Basic old\r\nUser-Agent: ua\r\n\r\n",
        );
        let text = String::from_utf8(forward_head(&h, Some("new"))).unwrap();
        assert!(!text.contains("Proxy-Connection"));
        assert_eq!(text.matches("Proxy-Authorization").count(), 1);
        assert!(text.contains("Proxy-Authorization: Basic new\r\n"));
        assert!(text.contains("User-Agent: ua\r\n"));
    }

    #[test]
    fn origin_form_adds_missing_host() {
        let h = head("GET http://example.com:8080/p HTTP/1.1\r\nAccept: */*\r\n\r\n");
        let text = String::from_utf8(origin_form_head(&h, "example.com", 8080)).unwrap();
        assert!(text.starts_with("GET /p HTTP/1.1\r\n"));
        assert!(text.contains("Host: example.com:8080\r\n"));
    }

    #[test]
    fn success_status_detection() {
        assert!(is_success_status(
            b"HTTP/1.1 200 Connection established\r\n\r\n"
        ));
        assert!(!is_success_status(b"HTTP/1.1 403 Forbidden\r\n\r\n"));
    }
}
