use super::*;
use crate::identity::IdentitySecret;

fn identity() -> LocalIdentity {
    LocalIdentity::from_secret(&IdentitySecret::generate().unwrap())
}

fn text(message: &Message) -> &str {
    match &message.body {
        MessageBody::Text { text } => text,
        MessageBody::Deleted => "",
        MessageBody::VoiceNote { .. } => "",
    }
}

#[test]
fn messages_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let saved;
    let first;
    {
        let mut store = Store::open(dir.path(), &LocalIdentity::from_secret(&secret)).unwrap();
        saved = store.saved_messages_id();
        first = store.insert_text(&saved, "первая заметка".into(), 1_000).unwrap();
        store.insert_text(&saved, "second".into(), 2_000).unwrap();
    }

    let store = Store::open(dir.path(), &LocalIdentity::from_secret(&secret)).unwrap();
    assert_eq!(store.saved_messages_id(), saved);
    let page = store.messages(&saved, None, 50).unwrap();
    assert!(!page.has_more);
    assert_eq!(page.messages.len(), 2);
    assert_eq!(page.messages[0], first);
    assert_eq!(text(&page.messages[1]), "second");
    assert!(page.messages[0].seq < page.messages[1].seq);
}

#[test]
fn bodies_are_not_stored_in_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    {
        let mut store = Store::open(dir.path(), &identity).unwrap();
        let saved = store.saved_messages_id();
        store
            .insert_text(&saved, "plaintext-marker-1234567890".into(), 1)
            .unwrap();
    }
    let account_dir = Store::account_dir(dir.path(), &identity.public().account_id);
    for entry in fs::read_dir(account_dir).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        let needle = b"plaintext-marker-1234567890";
        assert!(!bytes.windows(needle.len()).any(|w| w == needle));
    }
}

#[test]
fn second_open_of_same_account_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    let _store = Store::open(dir.path(), &identity).unwrap();
    assert!(matches!(Store::open(dir.path(), &identity), Err(Error::StorageLocked)));
}

#[test]
fn lock_is_released_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    drop(Store::open(dir.path(), &identity).unwrap());
    Store::open(dir.path(), &identity).unwrap();
}

#[test]
fn other_device_of_same_account_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    drop(Store::open(dir.path(), &LocalIdentity::from_secret(&secret)).unwrap());

    // Same account seed, different device seed.
    let mut bytes = secret.to_bytes().to_vec();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    let other_device = LocalIdentity::from_secret(&IdentitySecret::from_bytes(&bytes).unwrap());
    assert!(matches!(
        Store::open(dir.path(), &other_device),
        Err(Error::IdentityMismatch)
    ));
}

#[test]
fn paging_returns_older_messages() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path(), &identity()).unwrap();
    let saved = store.saved_messages_id();
    for i in 0..25 {
        store.insert_text(&saved, format!("m{i}"), i).unwrap();
    }

    let newest = store.messages(&saved, None, 10).unwrap();
    assert!(newest.has_more);
    let texts: Vec<_> = newest.messages.iter().map(text).collect();
    assert_eq!(texts.first(), Some(&"m15"));
    assert_eq!(texts.last(), Some(&"m24"));

    let older = store.messages(&saved, Some(newest.messages[0].seq), 10).unwrap();
    assert!(older.has_more);
    assert_eq!(text(&older.messages[0]), "m5");

    let oldest = store.messages(&saved, Some(older.messages[0].seq), 10).unwrap();
    assert!(!oldest.has_more);
    assert_eq!(oldest.messages.len(), 5);
    assert_eq!(text(&oldest.messages[0]), "m0");
}

#[test]
fn rejects_invalid_page_limits_and_unknown_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path(), &identity()).unwrap();
    let saved = store.saved_messages_id();
    assert!(store.messages(&saved, None, 0).is_err());
    assert!(store.messages(&saved, None, MAX_PAGE_SIZE + 1).is_err());

    let unknown = ConversationId::from_bytes([0; 16]);
    assert!(matches!(store.messages(&unknown, None, 10), Err(Error::NotFound(_))));
    assert!(matches!(
        store.insert_text(&unknown, "x".into(), 0),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn conversations_include_last_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path(), &identity()).unwrap();
    let saved = store.saved_messages_id();
    let conversations = store.conversations().unwrap();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].kind, ConversationKind::SavedMessages);
    assert!(conversations[0].last_message.is_none());

    let message = store.insert_text(&saved, "latest".into(), 5).unwrap();
    assert_eq!(store.conversation(&saved).unwrap().last_message, Some(message));
}

#[test]
fn tampered_body_is_reported_as_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    {
        let mut store = Store::open(dir.path(), &identity).unwrap();
        let saved = store.saved_messages_id();
        store.insert_text(&saved, "note".into(), 1).unwrap();
    }
    {
        let db = Store::account_dir(dir.path(), &identity.public().account_id).join(DATABASE_FILE);
        let conn = Connection::open(db).unwrap();
        conn.execute(
            "UPDATE messages SET body_ciphertext = zeroblob(length(body_ciphertext))",
            [],
        )
        .unwrap();
    }
    let store = Store::open(dir.path(), &identity).unwrap();
    let saved = store.saved_messages_id();
    assert!(matches!(store.messages(&saved, None, 10), Err(Error::Corrupted(_))));
}

#[test]
fn garbage_database_file_is_reported_as_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    let account_dir = Store::account_dir(dir.path(), &identity.public().account_id);
    fs::create_dir_all(&account_dir).unwrap();
    fs::write(account_dir.join(DATABASE_FILE), vec![0x42; 8192]).unwrap();
    assert!(matches!(Store::open(dir.path(), &identity), Err(Error::Corrupted(_))));
}

#[test]
fn newer_schema_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    drop(Store::open(dir.path(), &identity).unwrap());
    {
        let db = Store::account_dir(dir.path(), &identity.public().account_id).join(DATABASE_FILE);
        let conn = Connection::open(db).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    assert!(matches!(
        Store::open(dir.path(), &identity),
        Err(Error::UnsupportedStorageVersion(99))
    ));
}

#[cfg(unix)]
#[test]
fn account_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    drop(Store::open(dir.path(), &identity).unwrap());
    let mode = fs::metadata(Store::account_dir(dir.path(), &identity.public().account_id))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn profile_round_trips_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    {
        let mut store = Store::open(dir.path(), &LocalIdentity::from_secret(&secret)).unwrap();
        assert_eq!(store.profile().unwrap(), None);
        store.update_profile("Анна".into(), "".into(), 1).unwrap();
        store.update_profile("Анна К.".into(), "привет".into(), 2).unwrap();
    }
    let store = Store::open(dir.path(), &LocalIdentity::from_secret(&secret)).unwrap();
    let profile = store.profile().unwrap().unwrap();
    assert_eq!(profile.display_name, "Анна К.");
    assert_eq!(profile.about, "привет");
    assert_eq!(profile.updated_at_ms, 2);
}

#[test]
fn version_1_database_is_upgraded_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let identity = LocalIdentity::from_secret(&secret);
    let message = {
        let mut store = Store::open(dir.path(), &identity).unwrap();
        let saved = store.saved_messages_id();
        store.insert_text(&saved, "from v1".into(), 1).unwrap()
    };
    {
        // Reproduce a database written by schema version 1.
        let db = Store::account_dir(dir.path(), &identity.public().account_id).join(DATABASE_FILE);
        let conn = Connection::open(db).unwrap();
        conn.execute_batch(
            "DROP TABLE IF EXISTS room_members; \
             DROP TABLE IF EXISTS room_receipts; \
             DROP TABLE IF EXISTS pending_room_messages; \
             DROP TABLE IF EXISTS pending_room_ops; \
             DROP TABLE IF EXISTS rooms; \
             DROP TABLE IF EXISTS pending_message_ops; \
             DROP TABLE IF EXISTS voice_incoming; \
             DROP TABLE IF EXISTS voice_incoming_meta; \
             DROP TABLE IF EXISTS voice_notes; \
             ALTER TABLE messages DROP COLUMN revision; \
             ALTER TABLE messages DROP COLUMN edited_at_ms; \
             ALTER TABLE messages DROP COLUMN deleted; \
             DROP TABLE profile; DROP TABLE delivery_config; DROP TABLE contacts; \
             DROP TABLE invitations; DROP TABLE outbox; DROP TABLE processed_inbox; \
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let mut store = Store::open(dir.path(), &identity).unwrap();
    let saved = store.saved_messages_id();
    assert_eq!(store.messages(&saved, None, 10).unwrap().messages, vec![message]);
    assert_eq!(store.profile().unwrap(), None);
    store.update_profile("Upgraded".into(), "".into(), 3).unwrap();
}

#[test]
fn saved_message_edit_and_delete_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity();
    let saved;
    let id;
    {
        let mut store = Store::open(dir.path(), &identity).unwrap();
        saved = store.saved_messages_id();
        let message = store.insert_text(&saved, "черновик".into(), 1).unwrap();
        id = message.id;
        let edited = store
            .revise_own(&identity, &saved, &id, Some("готово".into()), 2)
            .unwrap();
        assert_eq!(text(&edited), "готово");
        assert_eq!(edited.revision, 1);
        assert_eq!(edited.edited_at_ms, Some(2));
        let same = store
            .revise_own(&identity, &saved, &id, Some("готово".into()), 3)
            .unwrap();
        assert_eq!(same.revision, 1);
        let deleted = store.revise_own(&identity, &saved, &id, None, 4).unwrap();
        assert!(deleted.deleted);
        assert_eq!(deleted.body, MessageBody::Deleted);
        assert!(store.revise_own(&identity, &saved, &id, Some("нет".into()), 5).is_err());
    }
    let store = Store::open(dir.path(), &identity).unwrap();
    let message = store.messages(&saved, None, 10).unwrap().messages.remove(0);
    assert_eq!(message.id, id);
    assert!(message.deleted);
    assert_eq!(message.body, MessageBody::Deleted);
    assert_eq!(message.revision, 2);
}
