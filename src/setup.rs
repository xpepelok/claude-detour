use crate::config::{Config, Protocol};
use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, Password, Select};
use std::path::Path;

pub fn run(path: &Path) -> Result<Config> {
    let theme = ColorfulTheme::default();
    println!("claude-detour setup. Arrow keys to choose, Enter to confirm.\n");

    let protocol = Protocol::ALL[Select::with_theme(&theme)
        .with_prompt("Proxy protocol")
        .items(&Protocol::ALL.map(|p| p.to_string()))
        .default(0)
        .interact()
        .context("reading protocol choice")?];

    let host: String = Input::with_theme(&theme)
        .with_prompt("Proxy IP or host")
        .validate_with(|s: &String| not_blank(s))
        .interact_text()
        .context("reading host")?;

    let default_port = match protocol {
        Protocol::Http => 8080,
        Protocol::Socks5 => 1080,
    };
    let port: u16 = Input::with_theme(&theme)
        .with_prompt("Proxy port")
        .default(default_port)
        .interact_text()
        .context("reading port")?;

    let (user, pass) = if Confirm::with_theme(&theme)
        .with_prompt("Does the proxy need a login and password?")
        .default(true)
        .interact()
        .context("reading auth choice")?
    {
        let user: String = Input::with_theme(&theme)
            .with_prompt("Login")
            .validate_with(|s: &String| not_blank(s))
            .interact_text()
            .context("reading login")?;
        let pass: String = Password::with_theme(&theme)
            .with_prompt("Password")
            .interact()
            .context("reading password")?;
        (Some(user), Some(pass))
    } else {
        (None, None)
    };

    let local_port: u16 = Input::with_theme(&theme)
        .with_prompt("Local listener port")
        .default(18080u16)
        .interact_text()
        .context("reading local port")?;

    let config = Config {
        protocol,
        host,
        port,
        user,
        pass,
        local_port,
        max_upstream: 6,
    };
    config.save(path)?;

    println!("\nSaved {}", path.display());
    println!(
        "The local proxy will listen on http://127.0.0.1:{} without a password",
        config.local_port
    );
    if config.credentials().is_some() {
        println!(
            "and reroute to {}://{}:{} with your login.",
            config.protocol, config.host, config.port
        );
    } else {
        println!(
            "and reroute to {}://{}:{}.",
            config.protocol, config.host, config.port
        );
    }
    println!("\nRun claude-detour with no arguments to launch Claude through it.");
    Ok(config)
}

fn not_blank(s: &str) -> std::result::Result<(), &'static str> {
    if s.trim().is_empty() {
        Err("cannot be empty")
    } else {
        Ok(())
    }
}
