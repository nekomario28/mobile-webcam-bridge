# Release process

`VERSION` is the single release version for the Android APK, Linux/Windows GUIs, AppImage, Setup.exe,
and release filenames. Releases use semantic `major.minor.patch` versions.

### Versions and editions, so "which build am I talking to" has an answer

Two numbers travel with an Android build, and both are derived from `VERSION` by
`android/app/build.gradle.kts`:

| | Derivation | `VERSION` 0.2.0 | the field's 0.1.4 |
|---|---|---|---|
| `versionName` | `VERSION` verbatim | `0.2.0` | `0.1.4` |
| `versionCode` | `major x 1_000_000 + minor x 1_000 + patch` | `2000` | `1004` |

So the field's `1004` and the repository's `2000` are the same scheme, not a discrepancy: the phone
was running the older *edition* of the same application. What was a real discrepancy, measured
2026-10-25, is in the release itself: **the `v0.2.0` release carries Linux and Windows 0.2.0 artifacts
but no Android 0.2.0 APK** — the only Android asset on it is `Mobile_Webcam-0.1.4-android.apk`. The
Android APK is produced by the release workflow after signing (`dist/Mobile_Webcam-$version-android.apk`)
and the signing step needs the one-time secrets below; with those unset, a release can be cut with
every other artifact and without a matching Android build. That is the whole of it, and it is why the
field could not be moved to 0.2.0 from the releases page.

## One-time Android signing setup

Create and securely retain one Android signing keystore. A later APK signed with a
different key cannot upgrade an installed release.

```bash
keytool -genkeypair -v \
  -keystore mobile-webcam-release.jks \
  -alias mobile-webcam \
  -keyalg RSA -keysize 4096 -validity 10000

gh secret set AMB_RELEASE_KEYSTORE_BASE64 < <(base64 -w0 mobile-webcam-release.jks)
gh secret set AMB_RELEASE_STORE_PASSWORD
gh secret set AMB_RELEASE_KEY_ALIAS
gh secret set AMB_RELEASE_KEY_PASSWORD
```

Keep an offline backup of the keystore and its credentials. Do not add them to Git.

## What the four Android signing secrets must be

The release workflow's Android step signs an APK and publishes
`dist/Mobile_Webcam-<version>-android.apk`; with the secrets unset it still finishes, which is how
`v0.2.0` shipped Linux and Windows artifacts and only `Mobile_Webcam-0.1.4-android.apk`.

| Secret | Value |
|---|---|
| `AMB_RELEASE_KEYSTORE_BASE64` | `base64 -w0 <your>.jks` |
| `AMB_RELEASE_STORE_PASSWORD` | the keystore password |
| `AMB_RELEASE_KEY_ALIAS` | the alias |
| `AMB_RELEASE_KEY_PASSWORD` | that alias's password |

**The keystore must be the one that signed the previous release**, or the new APK cannot upgrade an
installed one. Check before setting anything:

```
apksigner verify --print-certs Mobile_Webcam-<old>-android.apk | grep 'Signer #1 certificate'
# the release lineage so far: CN=Mobile Webcam, O=nekomario28, C=JP
# SHA-256 b907d74e82601ed23738ea6d82c30bad1549525d208d0268d7ba941ab20c69d9
```

A keystore that produces a different digest starts a different lineage: it installs beside the old app
rather than over it, and every future release has to use the same new key. Generate a replacement only as
a deliberate decision, recorded as one.

The plumbing is verified without a real key (measured 2026-10-25): with the four variables exported to a
throwaway keystore, `./gradlew :app:assembleRelease` produced `app-release.apk` **signed**
(`Signer #1 certificate DN: CN=Throwaway…`) where the unset case produces `app-release-unsigned.apk`. So
the workflow's secret→environment mapping and Gradle's signing config agree, and a release that fails on
signing will fail on the key, not on the wiring.

**A debug build cannot be upgraded by a release APK.** The field phone on 2026-10-25 was running
`CN=Android Debug` (`7972b85f…`), not the release lineage, so moving it to a release means an uninstall
first — which clears the app's own data (its approvals and Wi-Fi key), not the PC's.

## Release checklist

1. Update `VERSION` and `CHANGELOG.md` together.
2. Run host tests, Android `assembleRelease lintRelease`, the AppImage build and
   Windows build/package checks. Build the distributed Linux artifact on a
   generic x86_64 baseline, inspect its required glibc version and launch it on a
   different system to catch accidental host-library dependencies. Verify APK
   signing continuity and record the
   exact bundled dependency versions and corresponding source locations.
3. Review the change, complete available repository-native validation and record
   any unavailable CI or hardware gates before updating `main`.
4. Create and push the matching annotated tag from the exact `main` commit:

   ```bash
   version=$(cat VERSION)
   git tag -a "v$version" -m "Mobile Webcam $version"
   git push origin "v$version"
   ```

The `release` workflow rejects a tag that differs from `VERSION`, rebuilds and
verifies the signed APK, runs the host tests through the AppImage build, and then
publishes the Android/Linux artifacts with `SHA256SUMS` to GitHub Releases.
It does not build Windows.

If the self-hosted runner is unavailable, build from the exact tag locally using
the same Android signing key. Include the Windows Setup.exe produced by
[`windows/build-windows.sh`](../windows/build-windows.sh), source archives,
dependency inventories and a combined `SHA256SUMS`. Create a draft release,
verify every uploaded asset against its local hash and the tag against `main`,
then publish it. Cancel the queued workflow for that tag so it cannot later
publish a second release. Keep validation evidence under `docs/evidence/`;
local validation must not be described as CI success.

Hardware evidence remains separate from packaging. The release notes must describe
only the device gates recorded under `docs/evidence/` and keep unrun gates explicit.
