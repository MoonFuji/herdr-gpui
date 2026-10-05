use super::*;

fn lost(boot: &str, revision: u64) -> LiveState {
    let mut live = connected(boot, revision);
    live.status = ConnectionStatus::Disconnected;
    live.snapshot = None;
    live.surface = None;
    live
}

#[test]
fn a_held_frame_outlives_its_connection_until_a_new_frame_is_ready() {
    let mut presentation = Presentation::default();
    let live = connected("boot", 3);
    let first = live.surface.clone().unwrap();
    presentation.frame(&live);
    presentation.hold();
    assert!(presentation.stale());
    assert!(Arc::ptr_eq(
        &presentation.frame(&lost("boot", 3)).unwrap(),
        &first
    ));

    // The reconnected daemon's snapshot alone does not replace the picture.
    let mut handshaking = connected("boot", 9);
    handshaking.surface = None;
    assert!(Arc::ptr_eq(
        &presentation.frame(&handshaking).unwrap(),
        &first
    ));
    assert!(presentation.stale());

    let next = connected("boot", 9);
    let replacement = next.surface.clone().unwrap();
    assert!(Arc::ptr_eq(
        &presentation.frame(&next).unwrap(),
        &replacement
    ));
    assert!(!presentation.stale());
}

#[test]
fn a_held_frame_goes_when_the_daemon_restarted_or_the_window_clears_it() {
    let mut presentation = Presentation::default();
    presentation.frame(&connected("boot", 1));
    presentation.hold();
    let mut rebooted = connected("other-boot", 1);
    rebooted.surface = None;
    assert!(presentation.frame(&rebooted).is_none());
    assert!(!presentation.stale());

    presentation.frame(&connected("boot", 1));
    presentation.hold();
    presentation.clear();
    assert!(!presentation.stale());
    assert!(presentation.frame(&lost("boot", 1)).is_none());
}

#[test]
fn holding_nothing_is_not_stale() {
    let mut presentation = Presentation::default();
    presentation.hold();
    assert!(!presentation.stale());
    assert!(presentation.frame(&lost("boot", 1)).is_none());
}
