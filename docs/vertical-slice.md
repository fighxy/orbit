# Vertical slice: invites, text, edits, groups, channels, avatars, voice

Branch: `feat/vertical-chat`. One account is one device. The same Compose UI is
the Windows, Android, and iOS client. This note describes the working tree. It
is not a device test log, and it does not ask for a new MSI.

Meeting does not start by pasting a node address. After a display name is set,
the engine binds a direct Iroh endpoint and an `orbit://invite/…` link is
enough. A mailbox node is still available, and it is optional.

## How two people meet

1. Create an account (name, optional passcode). The secret stays in the OS
   secure store. The name is a local label on the profile and on the contact
   card. It is not a search key. There is no username directory.
2. Setting the name starts direct delivery, unless this account was already
   registered on a mailbox. The transport key is
   `data_dir/accounts/<account>/peer.key`. Its public half is the endpoint id
   put in invites, so it survives restart. The client shows "online" when that
   endpoint is bound. `Endpoint::online()` is not called: without a WAN it
   waits forever, and a same-machine session must still run.
3. Create an invite and send the `orbit://invite/…` text (at most 4096
   characters, 7 days). It is a device-signed contact card: account and device
   keys, inbox key, mailbox id, deposit token, display name, and the node
   field. On the direct path that field is a bare 64-hex endpoint id, not an
   IP. Sockets are not in the link.
4. The other person checks the invite, compares the full account key with the
   sender, and adds the contact. The signed reply adds the second account back
   on the author's side. An invite for one's own account is rejected. A second
   device of the same account is not a supported identity: there is no
   transfer and no multidevice.
5. Send text both ways while both apps are open. Incoming messages sit on the
   left, own messages on the right. "В очереди" is the local outbox.
   `delivered` is a checked receipt that the recipient stored the message, not
   a read receipt. The state name `mailbox` is also written after a direct
   peer acknowledges the envelope; the UI still labels that state "На узле".
   On the direct path that does not mean an `orbit-node` holds the ciphertext.

Direct sessions use ALPN `orbit/direct/1` and Iroh `presets::N0` (n0 discovery,
hole punch, then relay). A bare 64-hex id parses as a `NodeAddress` with no
sockets. Tests that run several engines in one process deliver through a
process-local map of loopback sockets. Two OS processes do not see that map
and would use n0. `direct_dial_by_id_over_n0` (only when `ORBIT_N0=1`) is a
dial by id, not a chat between two windows. It was not re-run for this note.

While an app is closed, direct delivery does not queue anywhere else. There is
no mailbox on the direct path. The sender keeps the sealed envelope in its own
outbox and retries only while its engine is running. The recipient gets
nothing until it is open and the dial succeeds.

`ORBIT_PROFILE=alice` and `ORBIT_PROFILE=bob` still give two desktop profiles
separate directories, keyring entries, and accounts.

## Mailbox path, unchanged in role

`hex@ip:port` and ALPN `orbit/mailbox/1` are still the store-and-forward path.
The mailbox client uses Iroh with relays disabled and dials the pasted sockets.
It does not use n0. Register from **Контакты и подключение** only when you
mean this path (**Свой узел в локальной сети**). Changing node after an invite
or a contact exists is rejected.

**Стать узлом** and `orbit-node host` turn this computer into that mailbox.
They publish a concrete IPv4 of the default route, not `0.0.0.0`, usually a
`192.168.x.x` address, plus a registration code stored next to the node key.
A phone on mobile data cannot route that address. The same Wi-Fi can. From
the public Internet the host is reachable only if UDP (default 7443) is
forwarded to it. The Windows helper tries to allow that UDP port; this note
does not claim a particular firewall rule. See
[deployment](../deploy/orbit-node/README.md).

`services/orbit-node/tests/vertical_chat.rs` still drives registration,
invite, queued text, mutual contact, both directions, an offline recipient,
and restart through a real QUIC mailbox. The outbox query now `LEFT JOIN`s
`rooms`. That test has not been re-run after the join, so this note does not
treat the mailbox scenario as freshly proven.

Android API 37 asks for nearby-devices access when registering on a node and
when accepting an invite. The in-app copy says a denial blocks a Wi-Fi node
and leaves a public node usable. That permission flow has not been checked on
a device.

`:apps:android:assembleDebug` wrote
`apps/android/build/outputs/apk/debug/android-debug.apk`
(`com.orbit.messenger` 0.1.0, minSdk 26, targetSdk 37, about 49 MB). It
contains `liborbit_ffi.so` for arm64-v8a and x86_64, Cargo dev profile. The
phone layout is the shared Compose UI: list, open chat, contacts, settings.
A gallery photo becomes a JPEG of at most 32 KiB. In a direct chat or in
saved messages the microphone records a voice note and the speaker plays it.
System back closes the open chat. The phone does not host a node. The package
was not installed, so none of that was tapped. A later source change encodes
a photo whose square side is already under 64 px. That change is not in the
assembled APK. Phrase registration is not on the screen.

## Edit and delete

Only the author can change a message. The change is a sealed payload,
`EditText` or `DeleteText` (postcard indexes 4 and 5; the enum is append-only).
Revision increases by one. A lower or equal revision is ignored. Delete is
terminal: a later edit does not restore the text. Saved messages use the same
local revision and are not sent.

If an edit or delete arrives before the text, it is held in
`pending_message_ops` (cap 64), not quarantined. A new early direct edit past
that cap is not stored, and the envelope is still accepted. It is not retried
and not quarantined.

Rooms do not reuse those payloads. `RoomEditText` and `RoomDeleteText` carry
the room id, so a room edit cannot apply to a direct chat.

## Groups and channels

Not MLS. There is no shared epoch. Each recipient gets a separate sealed copy
of the same payload (pairwise fan-out). A member who already decrypted a copy
keeps it. This is not a relay and not a channel that avoids a full mesh: every
other member is addressed. Cap 8 members including the creator, minimum 2.
Cap 100 rooms.

The creator picks the room id; the welcome repeats it, so every member shares
that id. `RoomWelcome` lists every member's contact card, including the
deposit token, so a mailbox member can still be deposited to. A member can
write to someone who was not a direct contact first, because the welcome
carried that card. The in-process test does this: Borya writes to Vera in a
group created by Anya, without a prior Borya–Vera direct chat.

A channel is the same fan-out with `can_post` only for the owner. Subscribers
cannot publish. Adding or removing a member is not implemented, so membership
is the welcome list. Someone who was not in it does not get history. There is
no backfill.

Text or an edit that arrives before the welcome is held (`pending_room_messages`
/ `pending_room_ops`, 64 rows each). Past that cap the sender gets a retry,
not a quarantine.

SQLite `CURRENT_VERSION` is 7. Version 5 adds `rooms`, `room_members`,
`room_receipts`, `pending_room_messages`, and `pending_room_ops`. Version 6
adds `profile.avatar` and the contact avatar columns. Version 7 adds
`voice_notes`, `voice_incoming_meta`, and `voice_incoming`. A version-1
database upgrades in place through these steps.

## Profile picture

A picture is JPEG (`FF D8 FF`) or PNG (`89 50 4E 47 0D 0A 1A 0A`), at most
32 KiB (`MAX_AVATAR_BYTES`). The command is `set_avatar`. An empty image
clears it. Postcard `Avatar` is index 10. It is its own envelope. The invite
stays at most 4096 characters (`MAX_INVITE_TEXT`) and does not carry the
bytes. Android scales a gallery image: EXIF orientation, long edge, center
square, then JPEG. Desktop does the same with ImageIO and a small EXIF
parser, from a file dialog parented to the window. `DesktopMediaTest` checks
orientation 6 and a small square JPEG. It does not open the window. The core
test `avatar_reaches_the_direct_contact_and_a_later_one_replaces_it` is in
the source. This note did not re-run it.

## Voice notes

A note is 16 kHz, mono, 16-bit PCM, WAV format tag 1. At most 2 MiB and
60 seconds (`MAX_VOICE_BYTES`, `MAX_VOICE_MS`). The core splits it into slices
of at most 32 KiB (`MAX_VOICE_CHUNK_BYTES`) so each sealed envelope stays
under 64 KiB. The receiver checks SHA-256 before the message appears.
Commands are `send_voice` and `read_voice`. Postcard indexes are `MediaStart`
11 and `MediaChunk` 12. The waveform is at most 48 bars. The codec is not
Opus.

Groups and channels return `InvalidArgument`: "voice notes are only in direct
chats and saved messages". Editing a body that is not text returns "only a
text message can be edited". Delete removes the stored WAV. A bad WAV is
rejected. `voice_note_round_trips_locally_and_reaches_the_contact` covers the
saved-message round trip, the rejected edit, delete, a bad WAV, and delivery
to a direct contact. This note did not re-run it.

Android records and plays through the platform microphone and speaker. That
path was not tapped. Desktop records and plays through `javax.sound.sampled`.
A line that opens at 32 or 48 kHz, or in stereo, is folded to 16 kHz mono
(left channel) on the way in and expanded on the way out. `DesktopMediaTest`
covers that fold. It does not open a microphone. `java.desktop` is on the
jpackage module list so a future installer keeps the mixer and ImageIO. No
new MSI was built. iOS hides the record and play controls, and avatar decode
there returns null.

## Recovery phrase

`IdentitySecret::generate_phrase`, `from_phrase`, and `recovery_phrase` live
in `crates/orbit-core`. Version 2 of the secret is one version byte plus 32
bytes of entropy. The 24 words encode only that entropy. Account and device
seeds are `blake3::derive_key` (`orbit 2026-10-01 phrase account seed v1` and
`phrase device seed v1`). When `peer.key` is missing, the direct endpoint key
is derived from the device seed, so a restored device keeps the endpoint id
already printed on old invites. Chats and contacts are not in the phrase.
Use it only after the previous device is gone: it brings that device back.
`phrase_restores_the_same_keys_and_hides_itself_from_a_random_secret` checks
the secret bytes and the public identity. It does not open two engines.

The screen does not show the words or a restore field. The FFI has no phrase
command. Onboarding still calls `IdentitySecret::generate`, the random
version-1 secret, which has no phrase. `normalize_username` trims to 4–31
characters of lowercase ASCII, digits, and underscore, and requires a digit.
It is not a directory and it is not the registration field. The registration
field is a free display name.

## UI in source

The shared Compose client has **Новая группа** and **Новый канал**: a dialog
for a title and the direct contacts that are already ready. A channel
subscriber does not get a composer; the line is «В этом канале публикует только
автор.» On a window at least 720 dp wide the list stays on the left and
settings, contacts, and «Стать узлом» open in the right pane. The list field
«Поиск по чатам» filters local titles and previews. Commands live in `shared/`
(`create_group`, `create_channel`, `edit_text`, `delete_text`, `set_avatar`,
`send_voice`, `read_voice`). This
documentation edit did not launch the window. Session tests stub those room
calls. Do not read them as a clicked UI pass.

Desktop records and plays a voice note through `javax.sound.sampled` and turns
a chosen image into a JPEG of at most 32 KiB. `DesktopMediaTest` covers the
32/48 kHz fold, stereo downmix, EXIF orientation 6, and a small square JPEG.
The test does not open a microphone or a window. iOS still hides record and
playback, and its avatar decode returns null. `java.desktop` is listed for a
future package so the mixer and ImageIO stay in the image. No new MSI was
built.

An installed desktop MSI can be older than this tree (ABI 5 or earlier). The
SDK starts only when the native library reports ABI 6. Building a new
installer is outside this note.

## What the core tests cover

Present in the tree when this slice landed. Not re-run while editing these
docs. `cargo clippy -p orbit-protocol -p orbit-core --all-targets -- -D warnings`
was green on that code.

- `two_accounts_exchange_text_by_invite_without_a_node`
- `author_edit_and_delete_reach_the_contact`
- `later_edit_wins_even_when_it_arrives_before_the_text`
- `saved_message_edit_and_delete_survive_reopen`
- `room_welcome_names_the_sender_and_round_trips`
- `three_accounts_share_a_group_and_a_channel` (three engines, one process:
  group, Borya writes to Vera without a prior direct chat, edit and delete,
  channel owner only, delete survives restart)
- `version_1_database_is_upgraded_in_place`
- `avatar_reaches_the_direct_contact_and_a_later_one_replaces_it`
- `voice_note_round_trips_locally_and_reaches_the_contact` (saved messages,
  rejected edit, delete, a bad WAV, then a direct contact)
- `phrase_restores_the_same_keys_and_hides_itself_from_a_random_secret`
- `username_follows_the_registration_rules` (the helper only; the screen does
  not ask for a username)

Device runs, push, and background delivery are not done. Platform CI builds
are not a substitute.

## Limits

- HPKE + Ed25519 is interim. No forward secrecy. MLS is not started.
- One account, one device. Do not copy a profile onto another device.
- No username search. No short link. The invite is the long `orbit://invite/…` text.
  The 24-word phrase is Rust-only. The screen and the FFI do not use it.
- Direct path: nothing is stored for a closed app. Mailbox offline delivery is
  a different path and was not re-proven after the rooms outbox join.
- Groups and channels: pairwise copies, not MLS, at most 8 members, no
  membership changes, no history for anyone absent from the welcome.
- Voice notes are 16 kHz mono 16-bit PCM WAV, at most 60 seconds, only in
  direct chats and saved messages. They are chunked under the 64 KiB envelope
  (`MAX_ENVELOPE_BYTES`) and checked with SHA-256 before the message appears.
  Not Opus. Groups and channels reject them. Desktop records and plays that
  WAV through the JVM mixer. iOS has no capture. Video notes and voice rooms
  are not started. There is no SFU.
- The node operator still sees IP, sizes, times, and mailbox ids. The node
  does not see direct-message plaintext.
- A bad or unauthorized inbox item is recorded and removed. A local storage
  failure does not ACK it.
