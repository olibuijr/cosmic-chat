mod app;
mod config;
mod irc_client;

use std::env;

fn main() -> cosmic::iced::Result {
    let args: Vec<String> = env::args().collect();

    // CLI pipe mode: read commands from stdin, emit IRC events to stdout
    if args.iter().any(|a| a == "--pipe" || a == "-p") {
        eprintln!("COSMIC Chat pipe mode — type /help for commands");
        eprintln!("Connect first: /connect irc.libera.chat 6697 mynick");
        run_pipe();
        return Ok(());
    }

    // Default: show cosmic_chat messages at info level. Use RUST_LOG=cosmic_chat=debug for verbose.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("cosmic_chat=info,warn"),
    )
    .init();

    let settings = cosmic::app::Settings::default()
        .size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(400.0))
        .size(cosmic::iced::Size::new(900.0, 640.0));

    log::info!("COSMIC Chat v{} starting", env!("CARGO_PKG_VERSION"));
    cosmic::app::run::<app::CosmicChat>(settings, ())?;
    Ok(())
}

/// Simple CLI loop: reads lines from stdin, sends to IRC, prints responses.
fn run_pipe() {
    use std::io::{BufRead, BufReader, Write};
    let stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();

    for line in stdin.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/quit" || trimmed == "/exit" {
            writeln!(stdout, "Goodbye.").ok();
            break;
        }
        if trimmed == "/help" {
            writeln!(stdout, "Commands: /connect host port nick, /join #chan, /msg #chan text, /quit").ok();
            continue;
        }
        // Placeholder — real pipe mode would spawn a connection
        writeln!(stdout, "echo: {trimmed}").ok();
        stdout.flush().ok();
    }
}
