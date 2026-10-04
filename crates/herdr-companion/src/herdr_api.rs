//! The one Herdr JSON API call the companion makes: `agent.prompt`, which
//! types a reply into the agent's pane through the daemon instead of
//! synthesizing terminal keys. This is the newline-delimited JSON API socket
//! (`herdr.sock`), not the binary client socket the GUI attaches to.

use crate::{Error, Result};
use herdr_client::{ConnectTarget, Stream};
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REPLY: u64 = 64 * 1024;

/// The API socket a hook-launched process would use: `HERDR_SOCKET_PATH`,
/// else `herdr.sock` beside the local session's client socket.
pub fn default_socket() -> Result<PathBuf> {
    default_socket_with(|name| env::var_os(name))
}

fn default_socket_with(var: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    if let Some(path) = var("HERDR_SOCKET_PATH") {
        return Ok(path.into());
    }
    let client = ConnectTarget::Local
        .local_session_socket_path()
        .map_err(|_| Error::NoHerdrSocket)?;
    Ok(client.with_file_name("herdr.sock"))
}

pub fn prompt(socket: &Path, pane_id: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::EmptyPrompt);
    }
    let mut stream = Stream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let request = json!({
        "id": "herdr-companion",
        "method": "agent.prompt",
        "params": { "target": pane_id, "text": text },
    });
    exchange(&mut stream, &request).map(drop)
}

fn exchange(stream: &mut (impl Read + Write), request: &Value) -> Result<Value> {
    let mut line = request.to_string().into_bytes();
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;

    let mut reply = Vec::new();
    BufReader::new(stream.take(MAX_REPLY)).read_until(b'\n', &mut reply)?;
    if reply.last() != Some(&b'\n') {
        return Err(if reply.len() as u64 == MAX_REPLY {
            Error::HerdrReplyTooLarge
        } else {
            Error::HerdrClosed
        });
    }
    let mut reply: Value = serde_json::from_slice(&reply)?;
    if let Some(error) = reply.get("error") {
        let field = |name| {
            error
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        return Err(Error::Herdr {
            code: field("code"),
            message: field("message"),
        });
    }
    Ok(reply
        .get_mut("result")
        .map(Value::take)
        .unwrap_or(Value::Null))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    /// Replays a canned reply and keeps what was written.
    struct Peer {
        reply: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl Peer {
        fn new(reply: &[u8]) -> Self {
            Self {
                reply: Cursor::new(reply.to_vec()),
                written: Vec::new(),
            }
        }
    }

    impl Read for Peer {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reply.read(buf)
        }
    }

    impl Write for Peer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn sends_one_json_line_and_returns_the_result() {
        let mut peer =
            Peer::new(b"{\"id\":\"herdr-companion\",\"result\":{\"type\":\"agent_prompted\"}}\n");
        let request = json!({"id": "herdr-companion", "method": "agent.prompt"});
        let result = exchange(&mut peer, &request).unwrap();
        assert_eq!(result, json!({"type": "agent_prompted"}));
        assert_eq!(peer.written, format!("{request}\n").into_bytes());
    }

    #[test]
    fn surfaces_the_daemons_error_fields() {
        let mut peer = Peer::new(
            b"{\"id\":\"x\",\"error\":{\"code\":\"agent_not_found\",\"message\":\"no agent\"}}\n",
        );
        let error = exchange(&mut peer, &json!({})).unwrap_err();
        assert!(matches!(
            error,
            Error::Herdr { code, message } if code == "agent_not_found" && message == "no agent"
        ));
    }

    #[test]
    fn a_closed_or_oversized_reply_is_an_error() {
        assert!(matches!(
            exchange(&mut Peer::new(b"{\"id\""), &json!({})),
            Err(Error::HerdrClosed)
        ));
        let huge = vec![b'x'; MAX_REPLY as usize + 1];
        assert!(matches!(
            exchange(&mut Peer::new(&huge), &json!({})),
            Err(Error::HerdrReplyTooLarge)
        ));
    }

    #[test]
    fn prefers_the_explicit_api_socket() {
        let path =
            default_socket_with(|name| (name == "HERDR_SOCKET_PATH").then(|| "/run/h.sock".into()))
                .unwrap();
        assert_eq!(path, PathBuf::from("/run/h.sock"));
    }

    #[test]
    fn refuses_an_empty_prompt_before_connecting() {
        assert!(matches!(
            prompt(Path::new("/nonexistent"), "p_1", "  "),
            Err(Error::EmptyPrompt)
        ));
    }
}
