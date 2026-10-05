//! Command-line parsing, kept pure over OS strings so it is testable and
//! exits before any socket is bound.

use serde_json::{Value, json};
use std::{ffi::OsString, net::SocketAddr, path::PathBuf, time::Duration};

pub const TOKEN_ENV: &str = "HERDR_COMPANION_TOKEN";
const DEFAULT_LISTEN: &str = "127.0.0.1:8787";
const DEFAULT_DECISION_SECS: u64 = 110;
/// Headroom between the companion giving up and Claude Code giving up, so the
/// companion always answers first and the terminal prompt appears cleanly.
const HOOK_HEADROOM_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    pub listen: SocketAddr,
    pub decision_timeout: Duration,
    pub herdr_socket: Option<PathBuf>,
    /// An ntfy topic URL that receives push notices.
    pub ntfy: Option<String>,
    /// Include commands and messages in push notices, not only their kind.
    pub ntfy_details: bool,
    /// Where the phone reaches the web app; tapped notices open it.
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Serve(ServeOptions),
    Hooks {
        url: String,
        decision_timeout: Duration,
    },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    #[error("unknown command or option `{0}`")]
    Unknown(String),
    #[error("`{0}` needs a value")]
    MissingValue(&'static str),
    #[error("invalid value for `{0}`")]
    InvalidValue(&'static str),
}

pub fn usage() -> String {
    format!(
        "herdr-companion: answer Claude Code prompts for Herdr agents from another device\n\n\
         USAGE:\n  \
         herdr-companion serve [--listen ADDR] [--decision-timeout SECS] [--herdr-socket PATH]\n  \
                               [--ntfy URL [--ntfy-details]] [--public-url URL]\n  \
         herdr-companion hooks [--url URL] [--decision-timeout SECS]\n\n\
         `serve` reads its bearer token from {TOKEN_ENV} and listens on {DEFAULT_LISTEN} by default.\n\
         It also serves the web app at /; open it on the phone and enter the token once.\n\
         `hooks` prints the Claude Code settings that point hooks at the companion.\n"
    )
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };
    let mut listen: SocketAddr = DEFAULT_LISTEN
        .parse()
        .map_err(|_| CliError::InvalidValue("--listen"))?;
    let mut url = None;
    let mut decision_timeout = Duration::from_secs(DEFAULT_DECISION_SECS);
    let mut herdr_socket = None;
    let mut ntfy = None;
    let mut ntfy_details = false;
    let mut public_url = None;
    let serve = match command.to_str() {
        Some("serve") => true,
        Some("hooks") => false,
        Some("help" | "-h" | "--help") => return Ok(Command::Help),
        _ => return Err(CliError::Unknown(command.to_string_lossy().into_owned())),
    };
    while let Some(arg) = args.next() {
        let mut value = |name| args.next().ok_or(CliError::MissingValue(name));
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Command::Help),
            Some("--decision-timeout") => {
                let secs = value("--decision-timeout")?
                    .to_str()
                    .and_then(|secs| secs.parse::<u64>().ok())
                    .filter(|secs| (1..=3600).contains(secs))
                    .ok_or(CliError::InvalidValue("--decision-timeout"))?;
                decision_timeout = Duration::from_secs(secs);
            }
            Some("--listen") if serve => {
                listen = value("--listen")?
                    .to_str()
                    .and_then(|addr| addr.parse().ok())
                    .ok_or(CliError::InvalidValue("--listen"))?;
            }
            // Socket paths need not be UTF-8.
            Some("--herdr-socket") if serve => {
                herdr_socket = Some(PathBuf::from(value("--herdr-socket")?))
            }
            Some("--ntfy") if serve => ntfy = Some(web_url(value("--ntfy")?, "--ntfy")?),
            Some("--ntfy-details") if serve => ntfy_details = true,
            Some("--public-url") if serve => {
                public_url = Some(web_url(value("--public-url")?, "--public-url")?)
            }
            Some("--url") if !serve => {
                url = Some(
                    value("--url")?
                        .into_string()
                        .map_err(|_| CliError::InvalidValue("--url"))?,
                );
            }
            _ => return Err(CliError::Unknown(arg.to_string_lossy().into_owned())),
        }
    }
    Ok(if serve {
        Command::Serve(ServeOptions {
            listen,
            decision_timeout,
            herdr_socket,
            ntfy,
            ntfy_details,
            public_url,
        })
    } else {
        Command::Hooks {
            url: url.unwrap_or_else(|| format!("http://{DEFAULT_LISTEN}/hooks/claude")),
            decision_timeout,
        }
    })
}

fn web_url(value: OsString, name: &'static str) -> Result<String, CliError> {
    value
        .into_string()
        .ok()
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .ok_or(CliError::InvalidValue(name))
}

/// Claude Code `settings.json` hooks for the companion. The token and pane id
/// come from the agent's environment at call time, so neither is written into
/// the settings file.
pub fn hooks_settings(url: &str, decision_timeout: Duration) -> Value {
    let handler = |timeout: u64| {
        json!([{
            "hooks": [{
                "type": "http",
                "url": url,
                "timeout": timeout,
                "headers": {
                    "Authorization": format!("Bearer ${TOKEN_ENV}"),
                    "X-Herdr-Pane": "$HERDR_PANE_ID",
                },
                "allowedEnvVars": [TOKEN_ENV, "HERDR_PANE_ID"],
            }]
        }])
    };
    json!({
        "hooks": {
            "PermissionRequest": handler(decision_timeout.as_secs() + HOOK_HEADROOM_SECS),
            "Notification": handler(5),
            "Stop": handler(5),
            "UserPromptSubmit": handler(5),
            "SessionStart": handler(5),
            // Progress during a turn. They run on every tool call, so they
            // return at once and never decide anything.
            "PreToolUse": handler(5),
            "PostToolUse": handler(5),
            "PostToolUseFailure": handler(5),
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, CliError> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn serve_defaults_to_loopback() {
        let Command::Serve(options) = parse(&["serve"]).unwrap() else {
            panic!("serve");
        };
        assert!(options.listen.ip().is_loopback());
        assert_eq!(
            options.decision_timeout,
            Duration::from_secs(DEFAULT_DECISION_SECS)
        );
        assert_eq!(options.herdr_socket, None);
        assert_eq!(options.ntfy, None);
    }

    #[test]
    fn parses_every_serve_option() {
        let command = parse(&[
            "serve",
            "--listen",
            "100.64.0.1:9000",
            "--decision-timeout",
            "30",
            "--herdr-socket",
            "/h.sock",
        ])
        .unwrap();
        assert_eq!(
            command,
            Command::Serve(ServeOptions {
                listen: "100.64.0.1:9000".parse().unwrap(),
                decision_timeout: Duration::from_secs(30),
                herdr_socket: Some("/h.sock".into()),
                ntfy: None,
                ntfy_details: false,
                public_url: None,
            })
        );
    }

    #[test]
    fn parses_push_options() {
        let Command::Serve(options) = parse(&[
            "serve",
            "--ntfy",
            "https://ntfy.sh/t",
            "--ntfy-details",
            "--public-url",
            "https://box.ts.net",
        ])
        .unwrap() else {
            panic!("serve");
        };
        assert_eq!(options.ntfy.as_deref(), Some("https://ntfy.sh/t"));
        assert!(options.ntfy_details);
        assert_eq!(options.public_url.as_deref(), Some("https://box.ts.net"));
        assert_eq!(
            parse(&["serve", "--ntfy", "ntfy.sh/t"]),
            Err(CliError::InvalidValue("--ntfy"))
        );
        assert_eq!(
            parse(&["hooks", "--ntfy-details"]),
            Err(CliError::Unknown("--ntfy-details".into()))
        );
    }

    #[test]
    fn rejects_bad_input_before_starting() {
        assert_eq!(
            parse(&["serve", "--listen"]),
            Err(CliError::MissingValue("--listen"))
        );
        assert_eq!(
            parse(&["serve", "--listen", "nope"]),
            Err(CliError::InvalidValue("--listen"))
        );
        assert_eq!(
            parse(&["serve", "--decision-timeout", "0"]),
            Err(CliError::InvalidValue("--decision-timeout"))
        );
        assert_eq!(
            parse(&["serve", "--url", "x"]),
            Err(CliError::Unknown("--url".into()))
        );
        assert_eq!(
            parse(&["hooks", "--listen", "x"]),
            Err(CliError::Unknown("--listen".into()))
        );
        assert_eq!(parse(&["launch"]), Err(CliError::Unknown("launch".into())));
        assert_eq!(parse(&[]), Ok(Command::Help));
        assert_eq!(parse(&["serve", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn hook_settings_give_the_companion_time_to_answer_first() {
        let settings = hooks_settings("http://h/hooks/claude", Duration::from_secs(60));
        let permission = &settings["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(permission["timeout"], 70);
        assert_eq!(settings["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 5);
        assert_eq!(
            permission["headers"]["Authorization"],
            "Bearer $HERDR_COMPANION_TOKEN"
        );
        assert_eq!(
            permission["allowedEnvVars"],
            json!(["HERDR_COMPANION_TOKEN", "HERDR_PANE_ID"])
        );
    }
}
