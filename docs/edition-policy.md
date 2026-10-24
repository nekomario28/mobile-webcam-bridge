# Edition policy (quality cap seam)

Shipped **OSS / Play free** binaries advertise and apply at most **1080p30**
(long edge ≤ 1920, short edge ≤ 1080, fps ≤ 30). **Play Pro** raises the cap
(4K, etc.) in the private `mobile-webcam-play` fork by replacing
`EditionPolicy.active` at process start.

| Build | Policy |
| --- | --- |
| Public `mobile-webcam-bridge` APK | `EditionPolicy.OssCap1080p30` (default) |
| Play free | same |
| Play Pro (entitled) | `EditionPolicy.Uncapped` or a Pro-specific policy |

Wire: `QUALITY_RESULT` reason **8 = edition-cap** when the device could run the
mode but this edition refuses it. Host UI continues to show only modes the phone
advertises, so a saved 4K wish simply fails to resolve until Pro unlocks.

Self-built OSS can still change the policy in source (Apache-2.0); the seam
defines what **we** distribute by default.
