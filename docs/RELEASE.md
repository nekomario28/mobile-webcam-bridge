# Release process

`VERSION` is the single release version for the Android APK, Linux/Windows GUIs, AppImage, Setup.exe,
and release filenames. Releases use semantic `major.minor.patch` versions.

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
