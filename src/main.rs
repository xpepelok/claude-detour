use anyhow::{Context, Result};
use claude_detour::config::{self, Config};
use claude_detour::{console_win, diagnostics, launcher, logger, proxy, setup};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = config::default_path();

    if args.is_empty() {
        return match launch(&path) {
            Ok(code) => code,
            Err(e) => {
                console_win::alert(&format!("{e:#}"));
                ExitCode::FAILURE
            }
        };
    }

    match cli(&args, &path) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cli(args: &[String], path: &Path) -> Result<ExitCode> {
    banner();
    match args[0].as_str() {
        "setup" | "config" => {
            logger::init(config::log_path(), true);
            setup::run(path)?;
            Ok(ExitCode::SUCCESS)
        }
        "test" | "--test" => {
            logger::init(config::log_path(), true);
            let cfg = load(path)?;
            Ok(ok_to_code(diagnostics::run(&cfg)))
        }
        "--forward-only" => {
            logger::init(config::log_path(), true);
            let cfg = Arc::new(load(path)?);
            proxy::run_foreground(cfg)?;
            Ok(ExitCode::SUCCESS)
        }
        "--help" | "-h" => {
            print_usage();
            Ok(ExitCode::SUCCESS)
        }
        other => {
            eprintln!("unknown argument: {other}\n");
            print_usage();
            Ok(ExitCode::FAILURE)
        }
    }
}

fn launch(path: &Path) -> Result<ExitCode> {
    let cfg = match Config::load(path) {
        Ok(cfg) => cfg,
        Err(_) => {
            logger::init(config::log_path(), true);
            println!("No config yet, let's create one.\n");
            setup::run(path)?
        }
    };

    logger::init(config::log_path(), false);
    console_win::hide();

    let cfg = Arc::new(cfg);
    let owns_listener = proxy::start_background(Arc::clone(&cfg))?;
    launcher::launch_through_proxy(&cfg)?;

    if owns_listener {
        launcher::wait_until_claude_exits();
    }
    Ok(ExitCode::SUCCESS)
}

fn load(path: &Path) -> Result<Config> {
    Config::load(path).with_context(|| {
        format!(
            "no usable config at {}, run `claude-detour setup` first",
            path.display()
        )
    })
}

fn ok_to_code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn banner() {
    use console::style;
    const INNER: usize = 38;
    let title = format!("{:<INNER$}", "  Claude Detour");
    let tagline = format!("{:<INNER$}", "  Claude Desktop through your proxy");
    let orange = 173;
    let rule: String = "\u{2500}".repeat(INNER);
    println!();
    println!("  {}", style(format!("\u{256d}{rule}\u{256e}")).dim());
    println!(
        "  {}{}{}",
        style("\u{2502}").dim(),
        style(title).color256(orange).bold(),
        style("\u{2502}").dim()
    );
    println!(
        "  {}{}{}",
        style("\u{2502}").dim(),
        style(tagline).dim(),
        style("\u{2502}").dim()
    );
    println!("  {}", style(format!("\u{2570}{rule}\u{256f}")).dim());
    println!();
}

fn print_usage() {
    println!(
        "claude-detour: run Claude Desktop through an HTTP or SOCKS5 proxy\n\n\
         USAGE:\n\
         \u{20} claude-detour              launch Claude through the configured proxy\n\
         \u{20} claude-detour setup        interactive proxy configuration\n\
         \u{20} claude-detour test         check that the proxy works\n\
         \u{20} claude-detour --forward-only   only run the local proxy, do not launch Claude\n\
         \u{20} claude-detour --help       show this help\n\n\
         Config and log live in %USERPROFILE%\\.claude-detour (config.toml, claude-detour.log)."
    );
}
