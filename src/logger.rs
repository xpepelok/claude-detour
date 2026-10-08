use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

struct Sink {
    path: Option<PathBuf>,
    echo: bool,
}

pub fn init(path: PathBuf, echo: bool) {
    let _ = SINK.set(Mutex::new(Sink {
        path: Some(path),
        echo,
    }));
}

pub fn line(msg: &str) {
    let stamped = format!("{}  {msg}\n", timestamp());
    let Some(sink) = SINK.get() else {
        return;
    };
    let Ok(sink) = sink.lock() else {
        return;
    };
    if sink.echo {
        eprint!("{stamped}");
    }
    if let Some(path) = &sink.path {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(stamped.as_bytes());
        }
    }
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("[{secs}]")
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::logger::line(&format!($($arg)*)) };
}
