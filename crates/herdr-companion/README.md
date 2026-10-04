# herdr-companion

A small headless bridge so another device, typically a phone, can work with the
coding agents running in Herdr without showing a terminal. You answer
permission prompts and `AskUserQuestion` forms with native controls, follow a
feed of what the agents are doing, and send replies.

Herdr still owns every terminal. The companion only adds hooks next to Herdr's
own integration. When nobody answers in time, Claude Code shows its usual
prompt in the pane.

```
Claude Code (in a Herdr pane) ──HTTP hooks──▶ herdr-companion ◀──HTTP── phone
                     ▲                              │
                     └──── agent.prompt ◀── Herdr JSON API socket
```

## Run it

```sh
export HERDR_COMPANION_TOKEN="$(openssl rand -hex 32)"   # also visible to agents
herdr-companion serve                                    # 127.0.0.1:8787
herdr-companion hooks > companion-hooks.json             # merge into ~/.claude/settings.json
```

`hooks` only prints settings and never edits your files. The printed hooks
send the token and the pane id from each agent's environment
(`HERDR_COMPANION_TOKEN`, `HERDR_PANE_ID`), so the settings file holds no secret.
Export the token where Herdr panes inherit it, for example in your shell profile.

The server speaks plain HTTP and binds to loopback by default. To reach it from
a phone, use an encrypted tunnel such as `tailscale serve`. If you bind to a
non-loopback address with `--listen`, the server prints a warning.

Options for `serve`:

- `--listen ADDR`: the address to bind, `127.0.0.1:8787` by default.
- `--decision-timeout SECS`: how long a hook waits for the phone before the
  prompt goes back to the terminal, 110 by default. `hooks` takes the same
  option and gives Claude Code's own timeout 10 seconds more.
- `--herdr-socket PATH`: Herdr's JSON API socket (`herdr.sock`), not the
  client socket. If you leave it out, the companion uses `HERDR_SOCKET_PATH`,
  then the local session's socket.

## API

Every route requires `Authorization: Bearer $HERDR_COMPANION_TOKEN`.

| Route | Purpose |
| --- | --- |
| `POST /hooks/claude` | Claude Code hook endpoint. It records `Notification`, `Stop`, `UserPromptSubmit` and other events. A `PermissionRequest` waits here for a decision. |
| `GET /v1/requests` | Prompts waiting for a decision, oldest first: `id`, `session_id`, `pane_id`, `cwd`, `tool_name`, `tool_input`. |
| `POST /v1/requests/{id}` | Decide a prompt. The decision shapes are listed below. |
| `GET /v1/events?after=N&wait=S` | Events after sequence `N`. With `wait`, it long-polls for up to 30 seconds. `next` is the cursor for the next call. `lost: true` means the bounded feed dropped events, so re-read `/v1/requests`. |
| `POST /v1/panes/{pane_id}/prompt` | `{"text": "..."}`. Herdr types the text into that agent's pane through `agent.prompt`. |

Decision bodies:

```jsonc
{"behavior": "allow"}                                   // optional updated_input, updated_permissions
{"behavior": "deny", "message": "use the staging DB"}  // Claude sees the message
{"behavior": "answer", "answers": {"Which DB?": "Postgres"}}  // AskUserQuestion only; lists for multi-select
{"behavior": "terminal"}                                // show the normal prompt in the pane instead
```

A request takes one decision. A late answer, after the timeout or after
someone else decided, gets a `404`.

## Limits and caveats

- While the companion is waiting, the pane shows no prompt; the terminal shows
  it only after `{"behavior": "terminal"}` or the timeout. Herdr's own hook
  still marks the agent as blocked, so the GUI still shows the attention dot.
- At most 64 prompts can wait at once, and the event feed keeps the last 512
  events. Request bodies are capped at 512 KiB. Text copied into the feed is
  cut at 16 KiB.
- Only Claude Code is wired up for now. Codex `hooks.json` and ACP sessions
  could post into the same broker.
- Windows builds and lints, but reply prompts there go through Herdr's named
  pipe and have not been exercised.
