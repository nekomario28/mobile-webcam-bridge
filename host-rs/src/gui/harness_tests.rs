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
