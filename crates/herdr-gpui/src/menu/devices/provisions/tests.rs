use super::*;
use crate::coder::BuildStatus;

#[test]
fn progress_reads_as_short_status_words() {
    assert_eq!(step_text(&Step::Creating), "Creating…");
    assert_eq!(step_text(&Step::Waiting(Progress::Starting)), "Starting…");
    assert_eq!(
        step_text(&Step::Waiting(Progress::Building(BuildStatus::Pending))),
        "Building (pending)…"
    );
    assert_eq!(step_text(&Step::CheckingHerdr), "Checking for Herdr…");
}
