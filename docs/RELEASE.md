# Release process

`VERSION` is the single release version for the Android APK, Linux GUI, AppImage,
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
2. Run the host tests, Android `assembleRelease lintRelease`, and AppImage build.
3. Merge the reviewed change to `main` with required CI checks complete.
4. Create and push the matching annotated tag from the exact `main` commit:

   ```bash
   version=$(cat VERSION)
   git tag -a "v$version" -m "Mobile Webcam $version"
   git push origin "v$version"
   ```

The `release` workflow rejects a tag that differs from `VERSION`, rebuilds and
verifies the signed APK, runs the host tests through the AppImage build, and then
publishes both artifacts with `SHA256SUMS` to GitHub Releases.

Hardware evidence remains separate from packaging. The release notes must describe
only the device gates recorded under `docs/evidence/` and keep unrun gates explicit.
