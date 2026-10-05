//! Coder workspaces being created or attached, owned by the window rather than
//! the dialog: a build can take minutes, so the dialog closes at once and each
//! job runs on its own worker. The device picker lists them with their latest
//! status; a toast reports each one ready (or failed). Closing the window
//! drops the jobs, which cancels them.

use crate::{
    HerdrWindow,
    coder::{
        Progress, SavedWorkspace, Settings,
        setup::{self, Source, Step},
        worker::{self, Worker},
    },
};
use gpui::Context;

/// The session every Coder device attaches to.
const SESSION: &str = "default";

enum Update {
    Step(Step),
    Installing,
    Finished(crate::coder::Result<SavedWorkspace>),
}

/// What a new job needs from the dialog.
pub(super) struct Request {
    pub(super) settings: Settings,
    pub(super) source: Source,
    pub(super) label: String,
    /// The user approved running Herdr's installer if the workspace lacks it.
    pub(super) install: bool,
}

pub(crate) struct Provision {
    id: u64,
    pub(crate) name: String,
    pub(crate) status: String,
    _job: Worker,
}

#[derive(Default)]
pub(crate) struct Provisions {
    jobs: Vec<Provision>,
    next: u64,
}

impl Provisions {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Provision> {
        self.jobs.iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.jobs.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Whether a job already targets this workspace name.
    pub(super) fn contains(&self, name: &str) -> bool {
        self.jobs.iter().any(|job| job.name == name)
    }
}

fn step_text(step: &Step) -> String {
    match step {
        Step::Creating => "Creating…".into(),
        Step::Waiting(Progress::Starting) => "Starting…".into(),
        Step::Waiting(Progress::Building(status)) => {
            format!("Building ({})…", format!("{status:?}").to_lowercase())
        }
        Step::CheckingHerdr => "Checking for Herdr…".into(),
    }
}

/// The whole job: create or attach, wait, install if approved and needed, save.
fn run(request: Request, cancelled: &dyn Fn() -> bool, send: &dyn Fn(Update)) {
    let Request {
        settings,
        source,
        label,
        install,
    } = request;
    let result = setup::provision(&settings, source, cancelled, |step| {
        send(Update::Step(step))
    })
    .and_then(|(ready, installed)| {
        if !installed {
            if !install {
                return Err(crate::coder::Error::Install(format!(
                    "Herdr is not installed in {}; add it and try again",
                    ready.name
                )));
            }
            send(Update::Installing);
            setup::install(&settings, &ready, cancelled)?;
        }
        let label = if label.is_empty() {
            &ready.name
        } else {
            &label
        };
        setup::save(&settings, &ready, label, SESSION)
    });
    send(Update::Finished(result));
}

impl HerdrWindow {
    /// Start a job for `request` named `name`; it reports through toasts.
    pub(super) fn start_provision(
        &mut self,
        name: String,
        request: Request,
        cx: &mut Context<Self>,
    ) -> crate::coder::Result<()> {
        let id = self.provisions.next;
        self.provisions.next += 1;
        let job = worker::spawn(
            "herdr-coder-provision",
            cx,
            move |cancelled, send| run(request, cancelled, send),
            move |this: &mut Self, update, cx| this.apply_provision(id, update, cx),
        )
        .map_err(|error| {
            tracing::error!(category = "coder_worker", error_kind = ?error.kind(), "Could not start Coder provision worker");
            crate::coder::Error::Worker("provision")
        })?;
        self.provisions.jobs.push(Provision {
            id,
            name,
            status: "Contacting Coder…".into(),
            _job: job,
        });
        cx.notify();
        Ok(())
    }

    fn apply_provision(&mut self, id: u64, update: Update, cx: &mut Context<Self>) {
        let Some(index) = self.provisions.jobs.iter().position(|job| job.id == id) else {
            return;
        };
        match update {
            Update::Step(step) => self.provisions.jobs[index].status = step_text(&step),
            Update::Installing => {
                self.provisions.jobs[index].status = "Installing Herdr…".into();
            }
            Update::Finished(result) => {
                let job = self.provisions.jobs.remove(index);
                match result {
                    Ok(saved) => self.local_transfer_notice(
                        "Coder workspace ready",
                        format!(
                            "{} is ready. Choose it in the device picker to start working.",
                            saved.label
                        ),
                        cx,
                    ),
                    Err(error) => self.local_transfer_notice(
                        "Coder workspace not added",
                        format!("{}: {error}", job.name),
                        cx,
                    ),
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
