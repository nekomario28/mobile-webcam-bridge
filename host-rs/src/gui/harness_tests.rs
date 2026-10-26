//! Host UI tests that drive the real widget tree, not a screenshot.
//!
//! Synthetic X11 clicks never reached egui in the capture gate (wheel events did),
//! so every interaction on the host side used to be verified by looking at pictures.
//! `egui_kittest` runs the actual `App` under AccessKit and lets a test press the
//! things it finds by name.

use super::*;
use egui::Vec2;
use egui_kittest::kittest::Queryable;

const PAIRED: &str = r#"[General]
decode="auto"
lanFingerprint="B12E094A4AE7C7DA6136FB69EE6685DA90CDC23E3F435FE4B90AC4ADDFE45CE4"
lanHost="192.168.1.14"
lanKey="B6D36CC758AA4AFF990D76324A7037A2"
output="/dev/video10"
transport="lan"
"#;

/// The real window size — a zero-sized viewport lays nothing out, and then nothing is
/// visible to AccessKit either. Steps are explicit because a running session asks for
/// a repaint every 100 ms by design, which an unbounded `run()` treats as a hang.
fn harness(settings: &str) -> (egui_kittest::Harness<'static, App>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("Mobile Webcam.conf");
    std::fs::write(&path, settings).expect("write settings");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
    let app_path = path.clone();
    let harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(420.0, 620.0))
        .build_eframe(move |cc| {
            let mut app = App::new_at(&cc.egui_ctx, app_path.clone());
            // English, so the assertions do not depend on the machine's locale.
            app.set_language(false);
            app
        });
    (harness, dir)
}

#[test]
fn a_paired_host_shows_the_pairing_and_hides_the_fields() {
    let (mut harness, _dir) = harness(PAIRED);
    harness.run_steps(3);
    assert!(
        harness.query_by_label_contains("Paired with").is_some(),
        "the main screen must say what it is paired with"
    );
    assert!(
        harness
            .query_by_label("Phone certificate SHA-256 (verify on phone)")
            .is_none(),
        "the pairing fields belong under Details once a pairing exists"
    );
}

#[test]
fn details_reveals_the_pairing_fields() {
    let (mut harness, _dir) = harness(PAIRED);
    harness.run_steps(3);
    harness.get_by_label("Details").click();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Phone certificate SHA-256 (verify on phone)")
            .is_some(),
        "opening Details must reveal the fields it hides"
    );
}

#[test]
fn a_revoked_pc_is_told_it_was_revoked_not_that_the_key_is_wrong() {
    // The bug this pins: every refusal used to read "access key rejected", which
    // sends the user to re-check 32 characters when the fix is one tap on the phone.
    // Unpaired on purpose: a complete pairing would auto-connect and replace the
    // injected state before it could be rendered.
    let unpaired = "[General]\ntransport=\"lan\"\noutput=\"/dev/video10\"\n";
    let (mut harness, _dir) = harness(unpaired);
    harness.run_steps(3);
    harness.state_mut().session.state =
        crate::session::State::Failed("this PC's access was revoked on the phone".into());
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Permission revoked on the phone")
            .is_some(),
        "a revoked PC must be told so"
    );
    assert!(
        harness
            .query_by_label_contains("Approve this PC again")
            .is_some(),
        "and told what to do about it"
    );
}

#[test]
fn a_dropped_usb_link_says_reconnecting_and_what_to_do() {
    // Unpaired USB: a cold start must not connect by itself, so the injected
    // failure is what the window renders. The retry is already armed; logic must
    // not arm it again (that would require pending or recover_from_drop).
    let settings = "[General]\ntransport=\"usb\"\noutput=\"/dev/video10\"\n";
    let (mut harness, _dir) = harness(settings);
    harness.run_steps(3);
    {
        let app = harness.state_mut();
        app.session.state = crate::session::State::Failed("peer disconnected".into());
        app.auto_retries = 1;
        app.auto_retry = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
        app.recover_from_drop = false;
        app.auto_attempt_pending = false;
        app.usb_wait_for_device = false;
        app.settings.transport = "usb".into();
    }
    harness.run_steps(2);
    assert!(
        harness.query_by_label_contains("Reconnecting").is_some(),
        "a scheduled retry is reconnecting, not a final failure"
    );
    assert!(
        harness
            .query_by_label_contains("The USB link dropped")
            .is_some(),
        "a USB drop must say what to do next"
    );
    harness.state_mut().set_language(true);
    harness.run_steps(2);
    assert!(
        harness.query_by_label_contains("再接続中").is_some(),
        "the same state in Japanese"
    );
}

#[test]
fn retries_that_ran_out_say_to_press_connect() {
    let unpaired = "[General]\ntransport=\"lan\"\noutput=\"/dev/video10\"\n";
    let (mut harness, _dir) = harness(unpaired);
    harness.run_steps(3);
    {
        let app = harness.state_mut();
        app.session.state = crate::session::State::Failed("connection refused".into());
        app.auto_retries = 9;
        app.auto_retry = None;
        app.recover_from_drop = false;
        app.auto_attempt_pending = false;
        app.usb_wait_for_device = false;
    }
    harness.run_steps(2);
    assert!(
        harness.query_by_label_contains("Press Connect").is_some(),
        "the budget is spent; the next action is a press"
    );
}

/// The device picker must carry an accessible name, and this must be checked where the
/// widget exists.
///
/// Measured 2026-10-26: the combo box only renders when the window has devices to list, so
/// on the CI runner (no USB hardware) the accessibility check never saw it, while a machine
/// that had one device reported "1 widget(s) lack accessible name ... ComboBox" and failed
/// `a_dropped_usb_link_says_reconnecting_and_what_to_do`. Two injected devices make the
/// widget's absence from CI impossible to keep.
#[test]
fn the_device_picker_has_an_accessible_name() {
    let settings = "[General]\ntransport=\"usb\"\noutput=\"/dev/video10\"\n";
    let (mut harness, _dir) = harness(settings);
    {
        let app = harness.state_mut();
        let device = |bus: u8, name: &str| crate::usb::Candidate {
            id: crate::usb::DeviceId {
                bus,
                address: 0x11,
                vid: 0x0fce,
                pid: 0x320d,
                ports: vec![1],
            },
            name: name.to_string(),
            suggested: false,
        };
        app.devices = vec![device(1, "Phone A"), device(2, "Phone B")];
        app.selected = None;
        app.settings.transport = "usb".into();
    }
    harness.run_steps(3);
    assert!(
        harness.query_by_label_contains("USB device").is_some(),
        "the picker must be reachable by its accessible name, not only on hover"
    );
}
