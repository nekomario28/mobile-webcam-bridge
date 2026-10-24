# Edition policy (quality cap seam)

Shipped **OSS / Play free** binaries advertise and apply at most **1080p30**
(long edge ≤ 1920, short edge ≤ 1080, fps ≤ 30). **720p60 is above the cap**
(fps). **Play Pro** raises the cap (4K, etc.) in the private
`mobile-webcam-play` fork by replacing `EditionPolicy.active` at process start —
without putting Pro/billing code in the public OSS tree.

| Build | Policy |
| --- | --- |
| Public `mobile-webcam-bridge` APK | `EditionPolicy.OssCap1080p30` (default) |
| Play free | same |
| Play Pro (entitled) | `EditionPolicy.Uncapped` or a Pro-specific policy |

## What the user sees

1. **Phone** enumerates camera + MediaCodec sizes/fps (`CaptureModes.supported`),
   then publishes `device ∩ EditionPolicy.active` (`CaptureModes.published` /
   `MainActivity` advertisement). Modes that would fail (unsupported size/fps
   or above the edition cap) are never advertised.
2. **Host UI** lists only modes the phone advertises — no hard-coded 4K/60 entry
   in the OSS picker. Prefer **hiding** above-cap options over a locked
   “Premium” row; a Premium mark is acceptable only if the edition seam can
   enable it in the private fork without OSS carrying billing code.
3. **Saved wish migration**: an old preference (e.g. 4K or 720p60) that does
   not resolve against the advertised list is rewritten to **Auto** (best
   allowed mode among published modes) and persisted, instead of failing the
   first apply. Explicit Apply of an unsupported wish still errors.

4. **Load-time clamp (host)**: when preferences load, a saved wish above the
   edition cap (long > 1920, short > 1080, or fps > 30) is clamped and written
   back **immediately**, so a 4K / 720p60 wish cannot linger and match a later
   phone that advertises Pro modes. Each field clamps independently to Auto:
   an over-cap size → size Auto (fps kept if allowed); fps > 30 → fps Auto
   (size kept if allowed); Auto then resolves to the best published mode when a
   phone connects. The clamp never invents a size the phone might not support.
   A failed write keeps the clamp in memory and says so in the warning; other
   settings and unknown keys are untouched.

The host cap is `quality::EditionCap`; shipped OSS uses `OSS_CAP_1080P30`
(`1920/1080/30`) via `quality::active_edition()`. The private fork changes that
one constant (or supplies its own `EditionCap`) to raise the cap — the clamp,
filter and tests all read it, and public OSS carries no Pro/billing code.

Wire: `QUALITY_RESULT` reason **8 = edition-cap** when the device could run the
mode but this edition refuses it (defense in depth if a stale host asks).

Host unit tests mirror the OSS cap in `host-rs` `quality::edition` for CI
without Android; the phone remains the source of truth for live capability
lists.

Self-built OSS can still change the policy in source (Apache-2.0); the seam
defines what **we** distribute by default.
