# Orbit implementation plan

Status: the plan from the 2026-10-01 architecture review, annotated for the
`feat/vertical-chat` tree. A checked item is in the working tree. An unchecked
item is not done. Checking a narrow slice does not close the stage criterion
under it. This file is not a test report and does not record an MSI rebuild.

Basis: [ADR 0001](adr/0001-rust-core-kmp-clients.md),
[architecture](architecture/rust-kmp.md),
[review of the original scaffold](reviews/2026-10-01-architecture-review.md).
Behavior of the current slice: [vertical slice](vertical-slice.md).

## 0. Reproducible and safe base

- [x] Pin the toolchain and a reproducible Gradle/Cargo build; add the wrapper and CI. _Rust 1.97, Gradle 9.8 with checksum, Kotlin 2.4.20, AGP 9.4.1. CI: Rust, JVM, Android, iOS._
- [x] Split JVM desktop and Kotlin/Native source sets; remove Java API from the Native target graph. _Native desktop targets removed. Shared `jniMain` for Android and JVM._
- [x] Create a minimal `crates/orbit-core/` and `crates/orbit-ffi/`.
- [x] KMP `NativeEngine` with Android/JVM JNI and iOS cinterop. _iOS compiles in CI. Not run on a device._
- [x] A minimal Compose screen and platform entry points. _Desktop and Android. Xcode host added. Not launched on a device._
- [x] Split public identity DTOs from secrets. No seed in sender or member models.
- [x] Replace Base64 key storage and TODOs with real secure-store adapters. No silent plaintext fallback. _Optional passcode (Argon2id) on top of the OS store._
- [x] Replace length-only storage with real storage owned by Rust.
- [x] Historical root `package.json` is not on the runtime path. _Moved to `tools/reference-holepunch/`._

Criterion: a clean build installs on an iOS device or simulator, on Android,
and on one desktop OS, calls Rust, and closes the engine. Storage round-trips
across restart. Smoke builds of the other declared targets are wired up.

Criterion state: desktop (Linux) was checked live for account creation,
messages, profile across restart, and engine close releasing the lock.
Android: the APK builds with `liborbit_ffi.so`; not run on a device. iOS: the
framework and Xcode host build in CI; a simulator or device run is still
required. Those statements are from stage 0, not a new run for this note.

## 1. Early architecture experiments

- [ ] Data transport: direct and relay, network changes, and reconnect. _The tree dials direct sessions with Iroh `presets::N0` (ALPN `orbit/direct/1`) and keeps mailbox QUIC on pasted sockets with relays off. Same-process tests use an in-process socket map. `direct_dial_by_id_over_n0` is a dial by id under `ORBIT_N0=1`, not a two-window chat, and was not re-run here. Network-change, CGNAT, and battery trials are not done._
- [ ] FFI ownership, cancel/close races, event wait, and snapshot recovery. _Stage 0 tests cover close, cancel, and `resync_required`. That is not a new device experiment._
- [ ] An E2EE candidate on two devices, crypto-state persistence, and crash recovery. _Interim HPKE is in the tree and has no forward secrecy and no ratchet. OpenMLS is not started._
- [ ] A voice experiment: three devices through a ready self-hosted SFU. _Not started. No SFU in the tree._
- [ ] Media adapters on iOS, Android, and desktop: devices, interruptions, Bluetooth, lifecycle. _Android and desktop record and play a voice note. iOS source records and plays and commits the take when the app backgrounds. That is not a device picker, an interruption trial, Bluetooth, or a run on a device. The Apple target was not compiled on Windows._
- [ ] Record chosen versions, results, and reasons in their own ADRs. _No transport or media ADR beyond ADR 0001._

Criterion: the expensive dependencies are confirmed by programs on real
targets. A missing desktop media binding or unreliable storage is settled
here, before a full UI and private infrastructure. This criterion is open.

## 2. Personal chat

- [x] One account and one device with a stable public identity. _No transfer. No multidevice. The display name is a label, not a lookup key._
- [x] A verified invite that pins the other party's identity. _`orbit://invite/…` is a signed contact card. On the direct path the node field is an endpoint id, not an IP. The recipient compares the account key._
- [x] A minimal chat list, history, and composer on KMP. _In `client/` and `shared/`. This note did not click the UI. Session tests do not drive the room dialog._
- [x] One transaction for the final ciphertext and the outbox. _HPKE envelopes. There is no MLS crypto state to commit._
- [x] Dedup, retry of the same envelope, durable receive, and ACK after commit. _On the direct path a closed app stores nothing. The sender retries from its outbox only while it is running._
- [ ] Logical streams per scope and device, with authorization to the scope. _Not implemented. Rooms are rows and pairwise envelopes, not per-device journals._
- [x] Mailbox with ciphertext, capability, TTL, and quotas. _`services/orbit-node`, `orbit/mailbox/1`. Optional next to direct delivery. HPKE has no forward secrecy. MLS stays a separate experiment. The mailbox integration test has not been re-run after the outbox `LEFT JOIN` on `rooms`._
- [x] Distinct states: local queue, accepted by the route, stored by the recipient. _`queued`, `mailbox`, `delivered`. On a direct dial, `mailbox` means the peer acknowledged storage, not that an `orbit-node` holds the bytes. The UI label is still "На узле"._

Criterion: a message crosses two devices by direct or relay and by offline
delivery; restart does not drop a confirmed message or rewrite a sealed
envelope; storage damage is not reported as success. Still open. Direct
delivery has no offline queue. The mailbox offline test was not re-proven
after rooms landed. Real phone lifecycle is not confirmed.

## 3. Persistent voice room

Not started. No SFU, no LiveKit, no str0m. An application envelope is at most
64 KiB (`MAX_ENVELOPE_BYTES`). A live room does not ride inside one text
envelope. Voice notes already split a WAV into 32 KiB slices. That path is
not a room.

- [ ] Community and a stored VoiceRoom description.
- [ ] A separate temporary VoiceSession and an authorization contract for join and speak.
- [ ] Join/leave, mute/deafen, audio device choice, speaking indication.
- [ ] A session ticket with freshness, reconnect, and end of a revoked session.
- [ ] File transfer must not block control and media.
- [ ] Background and audio lifecycle limits are visible in client state.

Criterion: three clients talk in one room, return after a drop, and survive
an audio-route change. Media E2EE is not claimed without a checked SDK and
key distribution. Nothing in this stage is implemented.

## 4. Channel and a basic community

The tree has a pairwise channel. That is not this stage. The owner is the
only publisher (`can_post`). Copies are sealed separately for each member,
at most 8 including the creator. There is no community, no role set beyond
that bit, no history for a member who was not in the welcome, and no way to
add a subscriber later. It is not "a channel without a full mesh".

- [ ] Separate Conversation, BroadcastChannel, Community, and VoiceRoom. _`Conversation.kind` is `saved_messages`, `direct`, `group`, or `channel`. Community and VoiceRoom do not exist._
- [ ] One policy controller and owner/publisher/subscriber roles. _A channel owner is the only publisher. There is no broader role model._
- [ ] Signed posts with a stable PostId; edit and delete as checked events. _Direct and room text use message ids and monotonic revisions. That is not a broadcast post log._
- [ ] Subscribe and load history without a full mesh of subscribers. _Not implemented. Delivery is fan-out._
- [ ] Permission checks in the core, on recipients, and on supporting services. _The core rejects a subscriber publish. There is no serving tier for a feed._

Criterion: the author publishes, a subscriber receives history, and cannot
publish as the author. The control stream has one authorized writer. Channel
privacy needs its own key scheme. Open. The pairwise cap and the missing
backfill do not meet it.

## 5. Attachments and recorded media

A voice note can cross a direct chat. It is a 16 kHz mono 16-bit PCM WAV,
at most 60 seconds, split into slices that fit the 64 KiB envelope. The
receiver checks the SHA-256 before storing it. The bubble shows duration and
a waveform. Android and desktop record and play that WAV in a direct chat and
in saved messages. Desktop uses the JVM mixer; that window was not clicked.
iOS source shows those buttons and records through AVAudioRecorder. That source was not compiled on Windows and was not run on a device. The codec is not Opus.
A video circle is not implemented. A profile picture is a separate JPEG or
PNG of at most 32 KiB; it does not ride inside the invite. A debug APK was
assembled and was not installed on a phone.

- [ ] File and photo first: durable manifest and blob, upload and download, cancel and resume, integrity.
- [ ] Then a voice note: capture, duration, waveform, playback. _Duration and waveform travel with the note, and a core test reassembles the WAV. Android has record and speaker playback in the client. That path was not tapped on a device. Desktop records and plays through javax.sound; the 32/48 kHz integer fold and stereo left-channel downmix are unit-tested. The window was not clicked and no new MSI was built. iOS source records and plays through AVFoundation and folds 32/48 kHz or stereo to 16 kHz mono. `:client:jvmTest` passed. The Apple target was not compiled on Windows, and no simulator or device was run._
- [ ] Then a video note: capture, preview, rotation, round mask.
- [ ] Codec and container checks on every target platform.
- [ ] Separate cache eviction, history retention, attachment keys, and backup.
- [ ] Limits on size, duration, resolution, and auto-download.

Criterion: an imported or recorded file plays after restart and after a
broken transfer. A temporary local file is not shown as an attachment others
can fetch. Previews follow the privacy policy of the source message. Open.

## 6. Private groups, devices, and recovery

The working group is pairwise fan-out of separate sealed copies. It is not
MLS: no shared epoch, no Commit or Welcome in the MLS sense, no membership
change, no history for a late member. A member who already decrypted a copy
keeps it. Do not treat that group as this stage.

- [ ] Define how `policy_version`, membership changes, and an MLS epoch relate.
- [ ] Fix authority, Commit/Welcome, late old-epoch events, and partition policy.
- [ ] Check device revoke, an unavailable controller, rejoin, and stale tickets.
- [ ] Design history for a new member and moving an account to a new device.
- [ ] Separate recovery of account authority from a backup of history and keys.
- [ ] Check a crash between decrypt or encrypt and a durable write of every related state.

Criterion: permissions and history converge under a written policy. Membership
in a cryptographic group alone cannot grant admin. Missing old keys do not
pretend that one network ciphertext still yields plaintext. Open.

## 7. Growth and operations

- [ ] Full product checks on Windows, Linux, and macOS beyond early smoke builds.
- [ ] Push, and a foreground resync that does not depend on background push succeeding.
- [ ] Threads on posts, richer roles, and moderation, each to its own requirements.
- [ ] Fuzz and limits; metrics for reconnect, battery, memory, storage, and traffic.
- [ ] Participant, file, and retention limits, and self-hosting modes. _The pairwise room cap (8 members, 100 rooms) is a code limit, not this product decision._

Criterion: operational limits are measured, and SFU, relay, and blob costs are
understood. Each earlier stage still has to close its own data and lifecycle
risks. Open. There is no SFU to measure.

## 8. Own infrastructure and library splits

- [ ] If a measured need exists, compare a Rust SFU or str0m with a checked baseline.
- [ ] Add sparse or Merkle replication only when measurements justify it.
- [ ] Extract crates from `orbit-core` along boundaries that have already held.
- [ ] Split node, mailbox, and push processes only when operations need it.

Criterion: a replacement passes the same client scenarios and has a concrete
advantage. The number of Holepunch packages ported is not a messenger metric.
Open. Orbit does not speak Holepunch on the wire. The
[component map](holepunch-map.md) is a reference.

## Current personal-chat slice

In `feat/vertical-chat`, on top of stage 0:

- Direct invite without a pasted node address. Signed card, endpoint id, ALPN
  `orbit/direct/1`, stable `peer.key`. Mailbox `hex@ip:port` remains optional.
  **Стать узлом** / `orbit-node host` is a same-Wi-Fi helper.
- Author edit and delete, including an edit that arrives before the text.
- Pairwise groups and channels in the core and in the Compose source. Not MLS.
  Max 8. No membership edits. No history for anyone not in the welcome. No
  queue for a closed app on the direct path.

Details and the core tests that cover this, and the mailbox test that was not
re-run, are in [vertical slice](vertical-slice.md). Stage 2's full criterion
stays open. Push and background delivery are not implemented. A voice note
is a chunked PCM WAV. The core test reassembles it in saved messages and in
a direct chat. Android and desktop record and play it. Those device paths
were not tapped, and the desktop window was not clicked. iOS source records and plays through AVFoundation. That source was not compiled on Windows and was not run on a device.
Video circles and voice rooms are not started. A profile picture is a JPEG
or PNG of at most 32 KiB, outside the invite. Schema version is 7. ABI is 6.
