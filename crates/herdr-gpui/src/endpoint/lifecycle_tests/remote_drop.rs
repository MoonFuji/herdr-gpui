use super::*;

#[test]
fn a_drop_is_an_outage_until_a_connection_has_a_snapshot() {
    let (mut endpoint, server) = connected_endpoint("ssh:remote");
    assert_eq!(endpoint.outage(), None);
    server.stream.shutdown(std::net::Shutdown::Both).unwrap();
    wait_until(|| {
        endpoint.poll(Instant::now());
        endpoint.connection.handle.is_none()
    });
    assert!(endpoint.outage().is_some());
    assert_eq!(endpoint.status(), "reconnecting");
    // A retry replaces `live` with a fresh attempt that has no error yet.
    endpoint.connect(ConnectOptions::default(), false);
    assert!(endpoint.live.error.is_none());
    assert!(endpoint.outage().is_some());
    assert_eq!(endpoint.status(), "reconnecting");

    let (mut endpoint, _server) = connected_endpoint("ssh:remote");
    endpoint.outage = Some("connection lost".into());
    endpoint.poll(Instant::now());
    assert_eq!(endpoint.outage(), None);
    assert_eq!(endpoint.status(), "online");
}

#[gpui::test]
fn a_dropped_remote_stays_selected_with_its_last_picture_dimmed(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (endpoint, server) = connected_endpoint("ssh:remote");
    view.update(cx, |view, _| {
        prepare_mouse(view, endpoint);
        assert!(view.presentation.picture(&view.live).is_some());
        assert!(!view.presentation.stale());
        view.endpoints[1].retry_at = Instant::now() + Duration::from_secs(120);
    });
    server.stream.shutdown(std::net::Shutdown::Both).unwrap();
    view.update(cx, |view, cx| {
        project_until(view, cx, "the drop", |view| {
            view.endpoints[1].connection.handle.is_none()
        });
        // An expired activation budget is renewed while there is no snapshot.
        view.activation_deadline = Some(Instant::now());
        view.poll_endpoints(cx);
        assert_eq!(
            view.selected_endpoint, 1,
            "a drop must not fall back to Local"
        );
        assert!(view.activation_deadline.unwrap() > Instant::now());
        assert!(view.presentation.stale());
        assert!(view.presentation.picture(&view.live).is_some());

        // Keys typed into the lost connection go nowhere, and say so.
        view.send(ClientPaneInputEvent::TextCommit("x".into()), cx);
        let (flash, _) = view.flash.as_ref().unwrap();
        assert!(flash.text.contains("Not connected"));
        assert_eq!(view.pending_input.len(), 0);

        // An automatic retry keeps the picture up while it connects.
        view.endpoints[1].retry_at = Instant::now();
        view.poll_endpoints(cx);
        assert_eq!(view.selected_endpoint, 1);
        assert_eq!(view.selected_generation, view.endpoints[1].generation);
        assert!(view.presentation.stale());
        assert!(view.presentation.picture(&view.live).is_some());

        // Leaving the endpoint leaves its picture behind.
        assert!(view.switch_endpoint(LOCAL, cx));
        assert!(!view.presentation.stale());
        assert!(view.presentation.picture(&view.live).is_none());
    });
}

#[test]
fn a_refusal_only_the_user_can_fix_waits_the_longest_retry_delay() {
    for (failure, waits_longest) in [
        (SshFailure::Auth, true),
        (SshFailure::HostKey, true),
        (SshFailure::HerdrMissing, true),
        (SshFailure::Unreachable, false),
    ] {
        let (mut endpoint, _server) = connected_endpoint("ssh:remote");
        let error = herdr_client::Error::SshRefused(failure).to_string();
        endpoint
            .connection
            .inbox
            .lock()
            .unwrap()
            .apply(ClientEvent::Disconnected {
                reason: error.clone(),
                ssh: Some(failure),
            });
        endpoint.connection.handle.as_ref().unwrap().disconnect();
        let now = Instant::now();
        endpoint.poll(now);
        let expected = if waits_longest {
            MAX_RETRY_DELAY
        } else {
            endpoint.retry_delay()
        };
        assert_eq!(endpoint.retry_at, now + expected, "{failure:?}");
        assert_eq!(endpoint.outage(), Some(error.as_str()));
    }
}

/// The worker stops the handle before its disconnect state reaches the inbox,
/// so a poll can find the connection gone while `live` still holds the old
/// snapshot. An activation budget must not run out in that gap either.
#[gpui::test]
fn a_drop_seen_before_its_state_arrives_does_not_fall_back_to_local(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (endpoint, _server) = connected_endpoint("ssh:remote");
    view.update(cx, |view, cx| {
        prepare_mouse(view, endpoint);
        view.endpoints[1].retry_at = Instant::now() + Duration::from_secs(120);
        // A stopped handle delivers no disconnect state at all.
        view.endpoints[1]
            .connection
            .handle
            .as_ref()
            .unwrap()
            .disconnect();
        view.activation_deadline = Some(Instant::now());
        view.poll_endpoints(cx);
        assert_eq!(
            view.selected_endpoint, 1,
            "a drop must not fall back to Local"
        );
        assert!(view.endpoints[1].connection.handle.is_none());
        assert!(view.live.snapshot.is_some(), "the gap under test");
        assert!(view.activation_deadline.unwrap() > Instant::now());
    });
}
