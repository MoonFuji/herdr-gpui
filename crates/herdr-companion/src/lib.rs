//! A companion bridge for agents running inside Herdr. Claude Code hooks post
//! here; a phone lists what is waiting, answers permission prompts and
//! `AskUserQuestion` forms, follows a bounded event feed, and sends replies
//! that Herdr types into the agent's pane. Herdr keeps owning every terminal;
//! when no answer arrives in time the prompt falls back to that terminal.

mod broker;
mod cli;
mod error;
mod herdr_api;
mod hook;
mod http;
mod server;

pub use broker::{
    Broker, Decision, Event, EventKind, EventPage, Limits, Origin, Outcome, PendingRequest,
};
pub use cli::{Command, ServeOptions, hooks_settings, parse_args, usage};
pub use error::{Error, HttpError, Result};
pub use herdr_api::{default_socket, prompt};
pub use server::{Companion, Config, serve};
