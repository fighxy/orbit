# Orbit documentation

Direction: Orbit's own protocol, a Rust core, Kotlin Multiplatform bridges and
clients, shared Compose UI.

Product aim: communities with persistent voice rooms, broadcast channels,
personal and group chats, files, voice messages, and video notes. The working
tree is earlier than that aim. What exists is direct text over an invite,
optional mailbox delivery, author edit and delete, pairwise groups and
channels (not MLS, at most 8 members), a profile JPEG or PNG of at most
32 KiB, and a voice note in a direct chat or in saved messages. Video notes,
file attachments, voice rooms, MLS, and an SFU are not started.

| Document | When to read it |
|---|---|
| [ADR 0001](adr/0001-rust-core-kmp-clients.md) | Chosen boundaries, and decisions that are still open |
| [Roadmap](roadmap.md) | What is in the tree and what the next stage still requires |
| [Architecture, revision 2](architecture/rust-kmp.md) | Target structure. Read the status note at the top before the design body |
| [Rust ↔ KMP bridge](architecture/kmp-bridge.md) | ABI 6, commands, memory rules, passcode |
| [Vertical slice](vertical-slice.md) | Invites, edits, groups, channels, avatars, voice notes, phrase limits |
| [Review of 2026-10-01](reviews/2026-10-01-architecture-review.md) | Why the strategy changed. A snapshot of the scaffold commit named in that file |
| [Holepunch map](holepunch-map.md) | Reference only. Orbit does not speak Holepunch on the wire |

An ADR records a decision. The architecture document is the target shape. The
roadmap is the order of work. The review is the tree it names, not this
branch. A document or a directory name is not a finished feature.

Stage 0 is in the tree: Rust core, C ABI/JNI, KMP SDK, Compose client, local
notes, profile, passcode. ABI is 6 (`ORBIT_ABI_VERSION` and
`SUPPORTED_ABI_VERSION`). Schema version is 7. The vertical slice adds direct
sessions (Iroh `presets::N0`, ALPN `orbit/direct/1`, endpoint id in the
invite), optional `orbit/mailbox/1` delivery, HPKE without forward secrecy,
edit and delete, pairwise groups and channels, avatars, and chunked voice
notes. Android records and plays a note and scales a gallery photo. Desktop
does the same through `javax.sound` and a file dialog; `DesktopMediaTest`
passed, the window was not clicked, and no new MSI was built. iOS source records and plays through AVFoundation and scales a chosen photo to a JPEG of at most 32 KiB. `:client:jvmTest` passed. The Apple target was not compiled on Windows, and no simulator or device was run. Onboarding is a display name and an optional passcode.
A 24-word phrase can restore the same Rust identity and is not on the screen
or in the FFI. An installed MSI from before ABI 6 will not load this SDK.
This index does not ask for a rebuild.

Device checks and load tests have not been done. The mailbox integration test
has not been re-run after the rooms outbox join. UI for groups and channels
is source in `client/` and `shared/`; this index does not claim the dialogs
were clicked.
