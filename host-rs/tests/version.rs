//! One version, one place.
//!
//! `docs/RELEASE.md` says `VERSION` is the single release version for the APK, both
//! GUIs and every artifact name, and the Android build derives from it. The Rust
//! crate does not, and nothing noticed when the two disagreed: at one point `VERSION`
//! said 0.1.4 while the host reported 0.2.0, which would have cut artifacts named
//! 0.1.4 around a 0.2.0 binary — and `release.yml` would have refused the tag before
//! anything was built.

#[test]
fn the_crate_version_is_the_project_version() {
    let version = std::fs::read_to_string("../VERSION")
        .expect("VERSION at the repository root")
        .trim()
        .to_string();
    assert!(
        !version.is_empty(),
        "VERSION must not be empty; the Android build derives versionCode from it"
    );
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        version,
        "host-rs/Cargo.toml and VERSION disagree: the release names artifacts from \
         VERSION while the binary reports Cargo's version"
    );
}

#[test]
fn the_field_build_can_be_mapped_back_to_a_version() {
    // The phone in the field reported versionName 0.1.4 with versionCode 1004. That
    // mapping is the Android build's own (major * 1e6 + minor * 1e3 + patch), so a
    // build identity from a screenshot or `dumpsys` can be decoded without guesswork.
    fn code(major: u32, minor: u32, patch: u32) -> u32 {
        major * 1_000_000 + minor * 1_000 + patch
    }
    assert_eq!(code(0, 1, 4), 1004);
    // And the project's version maps to the code the next build will carry.
    let version = std::fs::read_to_string("../VERSION")
        .expect("VERSION")
        .trim()
        .to_string();
    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    assert_eq!(
        parts.len(),
        3,
        "VERSION must be major.minor.patch: {version}"
    );
    assert!(
        code(parts[0], parts[1], parts[2]) > 1004,
        "the next build must out-rank the field one"
    );
}

/// Every lockfile in the repository that records this crate must record the version
/// in `VERSION`.
///
/// Measured 2026-10-26: `VERSION` went 0.2.0 -> 0.2.4 and `host-rs/Cargo.lock`
/// followed, but `host-rs/fuzz/Cargo.lock` kept `mobile-webcam 0.2.0`, so
/// `cargo metadata --locked` in the fuzz workspace refused to run on `main`. The
/// release workflow builds with `--locked`, so a lockfile left behind is a release
/// that cannot be cut - and the first test above could not see it, because it reads
/// the crate manifest rather than the files that pin the crate.
#[test]
fn every_lockfile_that_names_the_crate_agrees_with_version() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root");
    let version = std::fs::read_to_string(root.join("VERSION"))
        .expect("VERSION")
        .trim()
        .to_string();

    let mut checked = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // `target` holds gigabytes of generated locks; `.git` is not ours.
                if name != "target" && name != ".git" && name != "node_modules" {
                    stack.push(path);
                }
                continue;
            }
            if name != "Cargo.lock" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable lockfile");
            let mut lines = text.lines();
            while let Some(line) = lines.next() {
                if line != "name = \"mobile-webcam\"" {
                    continue;
                }
                let locked = lines
                    .next()
                    .and_then(|l| l.strip_prefix("version = \""))
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or_else(|| panic!("{}: malformed entry", path.display()));
                assert_eq!(
                    locked,
                    version,
                    "{} pins mobile-webcam {locked} but VERSION says {version}: \
                     `cargo build --locked` in that workspace will refuse to run",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 2,
        "expected host-rs/Cargo.lock and host-rs/fuzz/Cargo.lock to name the crate, saw {checked}"
    );
}
