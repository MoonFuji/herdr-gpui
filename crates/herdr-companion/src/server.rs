//! Routing and the accept loop. One thread per connection, bounded by
//! `max_connections`: a hook connection deliberately stays open while it waits
//! for the phone, so the cap is what keeps that waiting bounded.

use crate::{
    Error, Result,
    broker::{Broker, Decision, Limits},
    herdr_api,
    hook::{self, Hook},
    http::{self, Method, Request, Response},
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::BufReader,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_EVENT_WAIT: Duration = Duration::from_secs(30);

pub struct Config {
    /// Every route, hooks included, requires `Authorization: Bearer <token>`.
    pub token: SecretString,
    /// How long a hook waits for the phone before handing the prompt back to
    /// the terminal. Keep it below the hook's own `timeout`.
    pub decision_timeout: Duration,
    /// Herdr's JSON API socket for reply prompts; `None` disables them.
    pub herdr_socket: Option<PathBuf>,
    pub limits: Limits,
    pub max_connections: usize,
}

pub struct Companion {
    config: Config,
    broker: Broker,
}

#[derive(Deserialize)]
struct PromptBody {
    text: String,
}

impl Companion {
    pub fn new(config: Config) -> Self {
        Self {
            broker: Broker::new(config.limits),
            config,
        }
    }

    pub fn broker(&self) -> &Broker {
        &self.broker
    }

    fn authorized(&self, request: &Request) -> bool {
        let Some(presented) = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return false;
        };
        constant_time_eq(
            presented.as_bytes(),
            self.config.token.expose_secret().as_bytes(),
        )
    }

    pub(crate) fn handle(&self, request: &Request) -> Response {
        if !self.authorized(request) {
            return Response::json(401, &json!({ "error": "missing or wrong bearer token" }));
        }
        let segments: Vec<&str> = request.path.trim_matches('/').split('/').collect();
        let result = match (request.method, segments.as_slice()) {
            (Method::Post, ["hooks", "claude"]) => self.claude_hook(request),
            (Method::Get, ["v1", "requests"]) => Ok(Response::json(
                200,
                &json!({ "requests": self.broker.pending() }),
            )),
            (Method::Post, ["v1", "requests", id]) => self.decide(id, request),
            (Method::Get, ["v1", "events"]) => Ok(self.events(request)),
            (Method::Post, ["v1", "panes", pane, "prompt"]) => self.prompt(pane, request),
            (
                _,
                ["hooks", "claude"]
                | ["v1", "requests" | "events", ..]
                | ["v1", "panes", _, "prompt"],
            ) => {
                return Response::json(405, &json!({ "error": "method not allowed" }));
            }
            _ => return Response::json(404, &json!({ "error": "not found" })),
        };
        result.unwrap_or_else(|error| Response::error(status_for(&error), &error))
    }

    fn claude_hook(&self, request: &Request) -> Result<Response> {
        match Hook::parse(&request.body, request.header("x-herdr-pane"))? {
            Hook::Event { origin, kind } => {
                self.broker.record(origin, kind);
                Ok(Response::empty())
            }
            Hook::Permission {
                origin,
                tool_name,
                tool_input,
                tool_use_id,
            } => {
                let id = match self.broker.open(origin, tool_name, tool_input, tool_use_id) {
                    Ok(id) => id,
                    // Too many waiting: let the terminal prompt as usual.
                    Err(Error::PendingFull) => return Ok(Response::empty()),
                    Err(error) => return Err(error),
                };
                let output = self
                    .broker
                    .await_decision(id, self.config.decision_timeout)
                    .and_then(|(request, decision)| hook::permission_output(&request, decision));
                Ok(output.map_or_else(Response::empty, |output| Response::json(200, &output)))
            }
        }
    }

    fn decide(&self, id: &str, request: &Request) -> Result<Response> {
        let Ok(id) = id.parse() else {
            return Ok(Response::json(404, &json!({ "error": "not found" })));
        };
        let decision: Decision = serde_json::from_slice(&request.body)?;
        self.broker.decide(id, decision)?;
        Ok(Response::json(200, &json!({ "ok": true })))
    }

    fn events(&self, request: &Request) -> Response {
        let after = request
            .query("after")
            .and_then(|after| after.parse().ok())
            .unwrap_or(0);
        let wait = request
            .query("wait")
            .and_then(|wait| wait.parse().ok())
            .map_or(Duration::ZERO, Duration::from_secs)
            .min(MAX_EVENT_WAIT);
        Response::json(200, &json!(self.broker.events_after(after, wait)))
    }

    fn prompt(&self, pane: &str, request: &Request) -> Result<Response> {
        let socket = self
            .config
            .herdr_socket
            .as_deref()
            .ok_or(Error::NoHerdrSocket)?;
        let body: PromptBody = serde_json::from_slice(&request.body)?;
        herdr_api::prompt(socket, pane, &body.text)?;
        Ok(Response::json(200, &json!({ "ok": true })))
    }
}

fn status_for(error: &Error) -> u16 {
    match error {
        Error::UnknownRequest(_) => 404,
        Error::NotAQuestion(_) => 409,
        Error::PendingFull => 503,
        Error::Http(crate::HttpError::BodyTooLarge | crate::HttpError::HeadTooLarge) => 413,
        Error::Json(_) | Error::Http(_) | Error::EmptyDenyMessage | Error::EmptyPrompt => 400,
        Error::NoHerdrSocket
        | Error::Herdr { .. }
        | Error::HerdrClosed
        | Error::HerdrReplyTooLarge
        | Error::Io(_) => 502,
    }
}

/// Compares the whole token regardless of where the first difference is.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// Accepts connections until the listener fails. Each connection carries one
/// request; a hook's connection stays open until its decision or timeout.
pub fn serve(listener: TcpListener, companion: Arc<Companion>) -> Result<()> {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if active.fetch_add(1, Ordering::AcqRel) >= companion.config.max_connections {
            active.fetch_sub(1, Ordering::AcqRel);
            let busy = Response::json(503, &json!({ "error": "too many connections" }));
            let _ = stream
                .set_write_timeout(Some(IO_TIMEOUT))
                .and_then(|()| busy.write_to(&mut &stream));
            continue;
        }
        let companion = Arc::clone(&companion);
        let active = Arc::clone(&active);
        thread::spawn(move || {
            // Diagnostics for a failed connection belong to that client; the
            // server keeps serving others either way.
            let _ = connection(&companion, stream);
            active.fetch_sub(1, Ordering::AcqRel);
        });
    }
    Ok(())
}

fn connection(companion: &Companion, stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let response = match http::read_request(&mut BufReader::new(&stream)) {
        Ok(request) => companion.handle(&request),
        Err(error) => Response::error(status_for(&error), &error),
    };
    response.write_to(&mut &stream)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::broker::ASK_USER_QUESTION;
    use serde_json::Value;
    use std::io::{Read, Write};

    const TOKEN: &str = "test-token-0123456789abcdef";

    fn companion(decision_timeout: Duration) -> Arc<Companion> {
        Arc::new(Companion::new(Config {
            token: TOKEN.into(),
            decision_timeout,
            herdr_socket: None,
            limits: Limits::default(),
            max_connections: 8,
        }))
    }

    fn call(companion: &Companion, method: Method, target: &str, body: &Value) -> (u16, Value) {
        let auth = format!("Bearer {TOKEN}");
        let request = Request::new(
            method,
            target,
            &[("Authorization", &auth), ("X-Herdr-Pane", "p_2")],
            body.to_string().as_bytes(),
        );
        let response = companion.handle(&request);
        let body = if response.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&response.body).unwrap()
        };
        (response.status, body)
    }

    fn permission(tool_name: &str, tool_input: Value) -> Value {
        json!({
            "session_id": "s1", "cwd": "/w", "hook_event_name": "PermissionRequest",
            "tool_name": tool_name, "tool_input": tool_input
        })
    }

    /// Runs the hook call on its own thread, as Claude Code's connection would,
    /// and returns the request id once the phone can see it.
    fn hook_in_background(
        companion: &Arc<Companion>,
        body: Value,
    ) -> (u64, thread::JoinHandle<(u16, Value)>) {
        let hook = {
            let companion = Arc::clone(companion);
            thread::spawn(move || call(&companion, Method::Post, "/hooks/claude", &body))
        };
        loop {
            if let Some(request) = companion.broker().pending().first() {
                return (request.id, hook);
            }
            thread::yield_now();
        }
    }

    #[test]
    fn every_route_requires_the_token() {
        let companion = companion(Duration::ZERO);
        for (auth, status) in [(None, 401), (Some("Bearer wrong"), 401), (Some(TOKEN), 401)] {
            let headers: Vec<(&str, &str)> = auth
                .map(|auth| ("Authorization", auth))
                .into_iter()
                .collect();
            let response =
                companion.handle(&Request::new(Method::Get, "/v1/requests", &headers, b""));
            assert_eq!(response.status, status, "{auth:?}");
        }
        assert_eq!(
            call(&companion, Method::Get, "/v1/requests", &Value::Null).0,
            200
        );
    }

    #[test]
    fn a_phone_approval_reaches_the_waiting_hook() {
        let companion = companion(Duration::from_secs(30));
        let (id, hook) =
            hook_in_background(&companion, permission("Bash", json!({"command": "ls"})));

        let (status, listed) = call(&companion, Method::Get, "/v1/requests", &Value::Null);
        assert_eq!(status, 200);
        assert_eq!(listed["requests"][0]["pane_id"], "p_2");
        assert_eq!(listed["requests"][0]["tool_input"]["command"], "ls");

        let (status, _) = call(
            &companion,
            Method::Post,
            &format!("/v1/requests/{id}"),
            &json!({"behavior": "allow"}),
        );
        assert_eq!(status, 200);
        let (status, output) = hook.join().unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            output["hookSpecificOutput"]["decision"],
            json!({"behavior": "allow", "updatedInput": {"command": "ls"}})
        );
    }

    #[test]
    fn a_question_is_answered_from_the_phone() {
        let companion = companion(Duration::from_secs(30));
        let questions = json!([{"question": "Which DB?", "header": "DB", "options": [{"label": "Postgres", "description": ""}], "multiSelect": false}]);
        let (id, hook) = hook_in_background(
            &companion,
            permission(ASK_USER_QUESTION, json!({"questions": questions})),
        );
        let (status, _) = call(
            &companion,
            Method::Post,
            &format!("/v1/requests/{id}"),
            &json!({"behavior": "answer", "answers": {"Which DB?": "Postgres"}}),
        );
        assert_eq!(status, 200);
        let (_, output) = hook.join().unwrap();
        assert_eq!(
            output["hookSpecificOutput"]["decision"]["updatedInput"]["answers"]["Which DB?"],
            "Postgres"
        );
    }

    #[test]
    fn an_unanswered_prompt_falls_back_to_the_terminal() {
        let companion = companion(Duration::ZERO);
        let (status, body) = call(
            &companion,
            Method::Post,
            "/hooks/claude",
            &permission("Bash", json!({})),
        );
        assert_eq!(
            (status, body),
            (200, Value::Null),
            "an empty 200 is no decision"
        );
        let (_, events) = call(&companion, Method::Get, "/v1/events?after=0", &Value::Null);
        assert_eq!(events["events"][1]["outcome"], "timed_out");
    }

    #[test]
    fn events_are_recorded_and_paged() {
        let companion = companion(Duration::ZERO);
        let stop = json!({"session_id": "s1", "hook_event_name": "Stop", "last_assistant_message": "done"});
        assert_eq!(
            call(&companion, Method::Post, "/hooks/claude", &stop),
            (200, Value::Null)
        );
        let (status, page) = call(
            &companion,
            Method::Get,
            "/v1/events?after=0&wait=0",
            &Value::Null,
        );
        assert_eq!(status, 200);
        assert_eq!(page["next"], 1);
        assert_eq!(page["events"][0]["kind"], "stopped");
        assert_eq!(page["events"][0]["last_assistant_message"], "done");
        assert_eq!(page["events"][0]["pane_id"], "p_2");
    }

    #[test]
    fn bad_input_maps_to_client_errors() {
        let companion = companion(Duration::ZERO);
        assert_eq!(
            call(
                &companion,
                Method::Post,
                "/v1/requests/9",
                &json!({"behavior": "terminal"})
            )
            .0,
            404
        );
        assert_eq!(
            call(
                &companion,
                Method::Post,
                "/v1/requests/x",
                &json!({"behavior": "terminal"})
            )
            .0,
            404
        );
        assert_eq!(
            call(
                &companion,
                Method::Post,
                "/v1/requests/1",
                &json!({"behavior": "nope"})
            )
            .0,
            400
        );
        assert_eq!(
            call(
                &companion,
                Method::Post,
                "/hooks/claude",
                &json!({"no": "event"})
            )
            .0,
            400
        );
        assert_eq!(
            call(&companion, Method::Get, "/hooks/claude", &Value::Null).0,
            405
        );
        assert_eq!(call(&companion, Method::Get, "/nope", &Value::Null).0, 404);
        assert_eq!(
            call(
                &companion,
                Method::Post,
                "/v1/panes/p_1/prompt",
                &json!({"text": "hi"})
            )
            .0,
            502
        );
    }

    #[test]
    fn serves_a_hook_over_a_real_socket() {
        let companion = companion(Duration::ZERO);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || serve(listener, companion));

        let body = json!({"session_id": "s", "hook_event_name": "Notification", "message": "hi"})
            .to_string();
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "POST /hooks/claude HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
        assert!(reply.contains("Content-Length: 0\r\n"));
    }
}
