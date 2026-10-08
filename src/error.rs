use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("malformed request from client")]
    BadRequest,

    #[error("request target `{0}` is not a host:port")]
    BadTarget(String),

    #[error("upstream proxy closed the connection before replying")]
    UpstreamClosed,

    #[error("upstream HTTP proxy rejected the request: {0}")]
    HttpRejected(String),

    #[error("SOCKS5: {0}")]
    Socks5(String),
}

pub type Result<T> = std::result::Result<T, ProxyError>;
