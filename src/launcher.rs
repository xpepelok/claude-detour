use crate::config::Config;
use anyhow::{anyhow, Result};

#[cfg(windows)]
pub fn launch_through_proxy(cfg: &Config) -> Result<()> {
    use crate::log;
    use std::process::Command;
    use std::thread::sleep;
    use std::time::Duration;

    let exe = find_claude().ok_or_else(|| {
        anyhow!("Claude.exe not found in the Microsoft Store package or the usual install folders")
    })?;
    log!("claude: {}", exe);

    kill_claude();
    sleep(Duration::from_millis(1500));

    let local = format!("http://127.0.0.1:{}", cfg.local_port);
    Command::new(&exe)
        .arg(format!("--proxy-server={local}"))
        .arg("--proxy-bypass-list=localhost;127.0.0.1")
        .env("HTTP_PROXY", &local)
        .env("HTTPS_PROXY", &local)
        .env("NO_PROXY", "localhost,127.0.0.1")
        .spawn()
        .map_err(|e| anyhow!("starting Claude: {e}"))?;
    log!("claude started through {local}");
    Ok(())
}

#[cfg(windows)]
pub fn wait_until_claude_exits() {
    use std::thread::sleep;
    use std::time::Duration;

    sleep(Duration::from_secs(10));
    while count_claude() > 0 {
        sleep(Duration::from_secs(5));
    }
}

#[cfg(windows)]
fn find_claude() -> Option<String> {
    use std::path::Path;
    use std::process::Command;

    if let Ok(out) = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-AppxPackage -Name Claude).InstallLocation",
        ])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let candidate = Path::new(line.trim()).join("app").join("Claude.exe");
            if !line.trim().is_empty() && candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let program_files = std::env::var("ProgramFiles").unwrap_or_default();
    let candidates = [
        Path::new(&local).join("AnthropicClaude").join("claude.exe"),
        Path::new(&local)
            .join("Programs")
            .join("Claude")
            .join("Claude.exe"),
        Path::new(&program_files).join("Claude").join("Claude.exe"),
    ];
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn kill_claude() {
    use std::process::Command;
    let _ = Command::new("taskkill")
        .args(["/IM", "Claude.exe", "/F"])
        .output();
}

#[cfg(windows)]
fn count_claude() -> usize {
    use std::process::Command;
    let Ok(out) = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Claude.exe", "/NH", "/FO", "CSV"])
        .output()
    else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains("claude.exe"))
        .count()
}

#[cfg(not(windows))]
pub fn launch_through_proxy(_cfg: &Config) -> Result<()> {
    Err(anyhow!(
        "launching Claude Desktop is only supported on Windows"
    ))
}

#[cfg(not(windows))]
pub fn wait_until_claude_exits() {}
