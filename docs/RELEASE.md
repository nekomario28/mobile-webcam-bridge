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

## State of the release pipeline, measured 2026-10-25

Three facts decide how a release gets made right now, and none of them was true in the docs before:

1. **The `publish` job needs a self-hosted runner that is not registered.**
   `gh api repos/.../actions/runners` reports **0 runners**, while the job is pinned to
   `runs-on: [self-hosted, Linux, X64, mobile-webcam]`. Every `release` workflow run in the history is
   `cancelled` for this reason, and the Android APK on the `v0.2.0` release was **signed and uploaded by
   hand** — which is also why that release carried `Mobile_Webcam-0.1.4-android.apk` and no 0.2.0 build.
   Tagging a release before a runner exists queues a job that never starts.
2. **The signing secrets were never in this repository.** `gh api .../actions/secrets` was empty, as were
   environments and org secrets, so no earlier release was signed by CI.
3. **A new signing lineage was created on 2026-10-25**, because the keystore that produced the previous
   one (`CN=Mobile Webcam`, `SHA-256 b907d74e…`) is not on this machine and cannot be recovered. The new
   key is `CN=Mobile Webcam, O=nekomario28, C=JP`, `SHA-256 BA:2A:15:A5…`, RSA 4096, valid 10 000 days, alias
   `mobile-webcam`, and its four secrets are set on the repository. **The consequence was measured, not
   assumed**: installing the 0.2.0 APK over a 0.1.4-lineage install fails with
   `INSTALL_FAILED_UPDATE_INCOMPATIBLE: Existing package … signatures do not match`, and the app installs
   cleanly only after an uninstall. The one install that had to be replaced was a Waydroid emulator, so
   the cost was a dev loop, not a user.

   The keystore and its password live **outside this repository**, beside the work tree at
   `../mobile-webcam-release.jks` and `../.mobile-webcam-release.secret` (`0600`). **Back them up off this
   machine before cutting a release**: if they are lost, the lineage is lost again, and every future
   APK needs an uninstall to install.

### Both jobs run end to end (2026-10-26)

**`publish`** ran on `ubuntu-24.04` from `v0.2.1` onward: signed APK, AppImage, `SHA256SUMS`, the FFmpeg
source offer and the release itself, with every published digest verifying against the downloaded bytes.
The Android signing secrets, the pinned FFmpeg build, linuxdeploy through `APPIMAGE_EXTRACT_AND_RUN`, and
the publish step are proven there.

**`windows`** published its Setup.exe and zip for the first time at `v0.3.2`. Getting there took six tags,
and the failures were not six bugs:

1. bindgen panicked in the job because it was missing the three environment variables the green Windows
   job in `rust-host-ci.yml` sets. **A green job in one workflow is not the same job in another.**
2. `package_licenses.py` ended on `FileNotFoundError: fonts/OFL.txt`, then lost every crate's upstream
   texts behind a warning because msys2's POSIX python does not consider `D:\…` absolute
   (`Path("D:\\a\\b").is_absolute()` is `False`). It now resolves relative manifest paths and recognises a
   drive letter whatever interpreter reads it.
3. The checksum step failed for four tags with `sha256sum: 'UnityCaptureFilter32.dll'$'\r'`. The DLLs were
   downloaded correctly; **the repository had no `.gitattributes`**, so the Windows checkout turned the
   checksum file's names into `....dll\r`. A CRLF checkout would also have put a carriage return in the
   installer's version string, since `windows/package.cmake` reads `VERSION`. `.gitattributes` keeps text
   at LF (`gradlew.bat` declared as the exception) and the windows leg of `rust-host-ci.yml` fails if that
   is ever lost - proven by cloning the previous commit with `core.autocrlf=true`.
4. msys2 python received a Windows-form path from CMake and opened it as relative
   (`/d/a/…/D:/a/…/verify_distribution.py`). The verifier now runs from the job's shell with a
   `cygpath -u` path, the way `package_linux.sh` runs it.
5. `makensis` reported `!include: could not find` for a generated manifest at an absolute path that CMake's
   own `EXISTS` called present. The packaging script now carries its own delete macros, so NSIS is handed
   one file and includes nothing of ours.
6. The upload named `dist/SHA256SUMS`, which the Linux job owns: a literal path is not a glob, so
   `gh release upload` treated it as a pattern that matched nothing. The Windows job uploads its own
   artifacts plus `SHA256SUMS-windows.txt`.

**Ordering beats a timeout.** The two jobs used to run in parallel with the Windows job polling for the
release ten times, thirty seconds apart; `publish` needs about 25 minutes, so the window expired and the
upload failed with `release not found` after the installer had been built. `windows` now declares
`needs: publish`. The run costs wall-clock and buys a release that cannot half-publish.


### The hosted runner carries the release now, and why that is also the safer choice

`publish` runs on `ubuntu-24.04` rather than a self-hosted label, because this repository is public:
hosted minutes are free, everything the job needs installs there (`android-actions/setup-android`, the
pinned FFmpeg build, and linuxdeploy through `APPIMAGE_EXTRACT_AND_RUN=1` because hosted runners have no
FUSE, which `host-rs/tools/package_linux.sh` already sets), and a self-hosted Linux runner on a public
repository would let a fork's pull request execute on that machine while this workflow publishes with
`contents: write`.

**Build the release on the oldest glibc you support, not on a rolling distribution.** Measured
2026-10-25: the AppImage pipeline built end to end on this Arch machine and then stopped in
`host-rs/tools/verify_distribution.py` with `glibc requirement exceeds 2.39: (2, 44)` — the binary had
linked against this machine's glibc. On `ubuntu-24.04` (glibc 2.39) the same steps pass, which is why the
hosted runner is not merely convenient here: it is what makes the artifact installable on the systems the
gate names.

A release by hand, for the period before the runner existed, is what was done and works:

```
AMB_RELEASE_STORE_FILE=…/mobile-webcam-release.jks \
AMB_RELEASE_STORE_PASSWORD=… AMB_RELEASE_KEY_ALIAS=mobile-webcam AMB_RELEASE_KEY_PASSWORD=… \
  ./gradlew :app:assembleRelease          # produces app-release.apk, signed
apksigner verify --print-certs app-release.apk
cp app-release.apk /tmp/Mobile_Webcam-$(cat ../VERSION)-android.apk
gh release upload v$(cat ../VERSION) /tmp/Mobile_Webcam-$(cat ../VERSION)-android.apk
```

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
