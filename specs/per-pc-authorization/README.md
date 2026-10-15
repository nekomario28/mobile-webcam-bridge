# Per-PC authorization implementation

Requested scope: real identity/list persistence, phone approval/revocation UI,
and mandatory camera/control authorization over both USB and pinned TLS Wi-Fi.
This is not complete until all three slices below are integrated. No merge or
release is authorized by tests alone; the earlier independent static transport
review does not cover this protocol. Do not launch another independent model
review under the previously exhausted one-review authorization.

## Preserved contracts

- Phone owns approval and first camera start; PC owns quality selection.
- No auto-approval or bearer-key fallback, including an empty phone list.
- USB bootstrap needs no QR. USB descriptor strings are claimed metadata, not
  identities and not a reason to inherit camera consent.
- Retain pinned TLS and its access key for Wi-Fi bootstrap. Do not invent short
  comparison codes or weaken pin verification.
- One owner for approval state, shared by USB and Wi-Fi. One bounded pending
  request tied to its live session; stale UI actions must not approve a new one.
- Persist before granting. Corruption/read/write failure must never create a new
  identity or an empty successful authorization list. Report errors explicitly.
- Preserve quality transaction deadlines, Stop responsiveness and output-based
  Applied confirmation. No automatic retry or recovery.

## Slices and acceptance

### 1. Persistence boundary — implemented, platform runtime checks pending

Host: stable P-256 identity stored beside user settings. Unix uses restrictive
files and atomic no-replace publication; Windows stores only user-scope DPAPI
ciphertext. Concurrent first-use processes must return the same identity.
Missing, corrupt, unreadable and write-failed are different outcomes; no silent
rotation. Reuse existing DPAPI rather than creating another FFI owner.

Phone: bounded, validated versioned list codec and a transactional repository,
with Android Keystore AES-GCM persistence. Derive client ID from stored public
key; preserve immutable key bytes. Enforce UTF-8 byte bounds and approval/expiry
limits. Failed writes latch the repository closed rather than publishing grants.
Use instrumentation to check real Keystore round trips and tamper rejection;
pure JVM tests alone do not verify the Android storage provider.

Done when real filesystem/DPAPI tests, pure repository/codec tests and Android
storage instrumentation exist and their executed/unexecuted evidence is named.
Persistence is not startup integration until slice 3 consumes it.

### 2. Approval and revocation surface — not started

Phone lists trusted IDs and explicitly untrusted labels, status/expiry, revoke
and explicit renew actions. Pending approve/reject controls belong to one
session, with storage errors shown in-place. Never show success if persistence
failed. PC reports pending/authorized/expired/rejected and permits Stop while
waiting. Approval does not itself start the camera.

Gate: EN/JA, narrow/scaled screenshots and real control actions that change the
persisted state. Discover the available gxkit/browser/Android capture tools;
compilation or state fixtures alone are not visual acceptance. If an unbiased
screenshot check cannot be run under the current review authorization, name that
unverified boundary rather than declaring UI acceptance.

### 3. Wire proof and shared session gate — not started

Hard cutover for the unreleased auth scaffolding: no v1-auth compatibility
wrapper, no timeout-based feature detection. Keep quality's own wire contract
independent. Define public-key transfer plus a domain-separated transcript bound
to a fresh single-use per-session challenge, transport and label. Check exact
header bounds before allocation and strict UTF-8/result combinations in both
languages; pin one canonical fixture consumed by both.

Phone session state owns challenge, proof, pending and granted state. Both
inbound capture commands and outbound video submission check it, including stale
callbacks, expiry and live revocation. The Activity cannot bypass it. Reject old
clients instead of falling back. Describe USB's lack of protection from an active
physical intermediary honestly.

Gate: real TLS/socket peer tests for pending approval, explicit approval/reject,
wrong key/ID, changed transcript, replay, storage failure, expiry while streaming,
live revoke, old-peer rejection and Stop during pending/partial frames. Exercise
production entry points, not just the pure model. Android emulator verifies
actual server dispatch/storage; physical USB and native consumers remain separate
acceptance gates.

## Evidence and continuation

Base: `1b94c803982bfcedf8404b4945deb491258d3133`. Its crypto regression checks
are not evidence for this feature's new persistence or session enforcement.

Storage pass: local Kotlin suite (including instrumentation source compilation),
Rust default/no-default/capture suites and strict clippy, fmt, diff checks, and
Windows no-default cross-target compilation pass. Real Unix file tests exercise
concurrent creation and protected reads. Substituting an overwriting rename for
no-replace publication fails the concurrency test; exposing mutable key bytes
or disabling label checks fails the Kotlin repository tests. Android Keystore
storage and Windows DPAPI runtime checks have been added to existing CI but are
not yet confirmed for this pass.

### Next Agent Prompt

Continue with the public-key transfer and phone-owned transcript/session state
from slice 3, then wire the slice 2 approval surface and both production
transports together. A usable pending request depends on that session owner;
do not add a disconnected list UI or enable a gate without a way to approve.
Use the new stores instead of reintroducing callback-only persistence. Verify
slice 1's native CI results as they arrive, and keep the PR Draft. The requested
feature is unfinished until UI, live revocation and mandatory gates consume the
stores in both transports. See [choices](choices.md) for current trade-offs.
