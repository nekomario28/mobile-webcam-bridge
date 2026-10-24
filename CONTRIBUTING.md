# Contributing

## License of contributions

By submitting a contribution you agree it is licensed under the same
[Apache License 2.0](LICENSE) as this repository (see Apache §5). The project
may produce proprietary Play builds and other edition-gated binaries that
include your contribution; do not submit code you cannot license that way.

## Developer Certificate of Origin (DCO)

Every commit must include a `Signed-off-by` line matching the commit author:

```
Signed-off-by: Your Name <you@example.com>
```

Use `git commit -s` (or `--signoff`). The `dco` GitHub Actions workflow rejects
pull requests that omit it.

The DCO text is the standard [developercertificate.org](https://developercertificate.org/)
certificate: you assert you have the right to submit the work under the
project's license.

## Pull requests

- Target `main`.
- Keep secrets, AdMob App IDs, and Play Billing keys out of this public repo.
- Do not import GPL/AGPL application source (see [docs/provenance.md](docs/provenance.md)).
- Prefer small, reviewable commits with clear messages.

## Local checks

- Android: `cd android && ./gradlew test`
- Rust host: follow `host-rs/README.md` (pinned Rust + minimal FFmpeg).
- `python3 host-rs/tools/check_ffmpeg_source_offer.py` when touching FFmpeg pins.
