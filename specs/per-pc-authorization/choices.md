# Implementation choices

These are implementing-agent choices, not independent security approval.

## Provisional — revisit at transport integration

- **Authorization-list failure latches the repository closed.** A failed Android
  preferences commit can still change the platform's memory cache. Keeping the
  last in-memory grant usable would make failure look like success. The session
  gate must treat repository failure as terminal, stop capture, and show the
  storage error. Restart is not proof that an unsuccessful revoke reached disk.
- **Revocation removes an entry.** The current model consequently treats its next
  valid proof as a new pending request. The gate must bound/cool down those
  requests; removal is not a permanent block list.

## Adopted for the storage slice

- **Windows identity is a DPAPI-protected file, not a new registry value.** This
  preserves same-user/machine protection through the existing DPAPI owner while
  allowing the same atomic no-replace publication as Unix. No settings read or
  write serializes the identity. Runtime Windows CI is still required.
- **A binary phone-list format instead of JSON.** The list is internal bounded
  storage, not a user-edited config or external API. A short versioned record
  avoids an extra JSON dependency/platform-stub issue and lets bounds be checked
  before allocating labels/list entries. It stores the public point and derives
  the ID, rather than persisting redundant values that could disagree.
- **Separate Keystore alias and AEAD context for the list.** Reusing the existing
  Wi-Fi secret's ciphertext format would entangle migration and recovery policy.
  Missing decryption keys fail without creating a replacement. No new crypto
  primitive or third-party dependency is introduced.
- **No silent label truncation or mutable key aliases.** The UTF-8 byte limit is
  enforced at approval/storage, and records copy inputs and return copied key
  bytes. Control/format characters are refused so an untrusted label cannot
  introduce extra lines or bidi formatting into the eventual approval surface.
