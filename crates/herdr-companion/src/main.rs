use anyhow::{Context, bail};
use herdr_companion::{Command, Companion, Config, Limits, hooks_settings, parse_args, usage};
use std::{env, net::TcpListener, process::ExitCode, sync::Arc};

/// A guessable token would let anyone who reaches the port approve commands.
const MIN_TOKEN_LEN: usize = 24;
const MAX_CONNECTIONS: usize = 128;

fn main() -> ExitCode {
    let command = match parse_args(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("herdr-companion: {error}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("herdr-companion: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> anyhow::Result<()> {
    let options = match command {
        Command::Help => {
            print!("{}", usage());
            return Ok(());
        }
        Command::Hooks {
            url,
            decision_timeout,
        } => {
            println!("{:#}", hooks_settings(&url, decision_timeout));
            return Ok(());
        }
        Command::Serve(options) => options,
    };
    let token = env::var("HERDR_COMPANION_TOKEN").context("HERDR_COMPANION_TOKEN is not set")?;
    if token.len() < MIN_TOKEN_LEN {
        bail!("HERDR_COMPANION_TOKEN must be at least {MIN_TOKEN_LEN} characters");
    }
    let herdr_socket = match options.herdr_socket {
        Some(path) => Some(path),
        None => herdr_companion::default_socket().ok(),
    };
    let listener = TcpListener::bind(options.listen)
        .with_context(|| format!("could not listen on {}", options.listen))?;
    if !options.listen.ip().is_loopback() {
        eprintln!(
            "herdr-companion: warning: {} is not loopback and this server speaks plain HTTP; \
             expose it through an encrypted tunnel such as Tailscale",
            options.listen
        );
    }
    eprintln!("herdr-companion: listening on {}", options.listen);
    let companion = Companion::new(Config {
        token: token.into(),
        decision_timeout: options.decision_timeout,
        herdr_socket,
        limits: Limits::default(),
        max_connections: MAX_CONNECTIONS,
    });
    herdr_companion::serve(listener, Arc::new(companion))?;
    Ok(())
}
