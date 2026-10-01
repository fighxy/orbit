use super::*;
use crate::identity::{IdentitySecret, LocalIdentity};
use orbit_protocol::mailbox::{Item, ItemId};

fn item(bytes: Vec<u8>) -> Item {
    Item {
        id: ItemId::of(&bytes),
        envelope: bytes,
        seq: 1,
        received_at_ms: 10,
        expires_at_ms: i64::MAX,
    }
}

fn setup(dir: &std::path::Path, name: &str) -> (Store, LocalIdentity) {
    let identity = LocalIdentity::from_secret(&IdentitySecret::generate().unwrap());
    let mut store = Store::open(dir, &identity).unwrap();
    store.update_profile(name.into(), "".into(), 1).unwrap();
    let address = format!("{}@127.0.0.1:7443", "ab".repeat(32));
    store.configure_delivery(&address).unwrap();
    store.mark_registered(&address).unwrap();
    (store, identity)
}

#[test]
fn outbox_survives_restart_exactly_and_inbox_deduplicates() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let (mut a, ai) = setup(a_dir.path(), "Алиса");
    let (mut b, bi) = setup(b_dir.path(), "Борис");
    let invitation = a.create_invite(&ai, 10).unwrap();
    let contact = b.accept_invite(&bi, &invitation, 11).unwrap();
    let request = b.outbox(10).unwrap().remove(0);
    a.receive_item(&ai, &item(request.envelope), 12).unwrap();
    b.deposited(&request.id).unwrap();
    let confirmation = a.outbox(10).unwrap().remove(0);
    b.receive_item(&bi, &item(confirmation.envelope), 13).unwrap();
    a.deposited(&confirmation.id).unwrap();
    let message = b
        .queue_text(&bi, &contact.conversation_id, "secret text".into(), 14)
        .unwrap();
    let before = b.outbox(10).unwrap().remove(0);
    drop(b);
    let mut b = Store::open(b_dir.path(), &bi).unwrap();
    let after = b.outbox(10).unwrap().remove(0);
    assert_eq!(before.envelope, after.envelope);
    assert_eq!(before.id, after.id);
    assert!(b.contact(&contact.conversation_id).unwrap().unwrap().ready);
    let incoming = item(after.envelope);
    let changes = a.receive_item(&ai, &incoming, 15).unwrap();
    assert_eq!(changes.messages.len(), 1);
    let first_receipt = a.outbox(10).unwrap().remove(0).envelope;
    let changes = a.receive_item(&ai, &incoming, 16).unwrap();
    assert!(changes.messages.is_empty());
    assert_eq!(a.outbox(10).unwrap().remove(0).envelope, first_receipt);
    let page = a.messages(&contact.conversation_id, None, 50).unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].id, message.id);

    // A failed outbox insert cannot leave a supposedly queued local message.
    b.conn
        .execute_batch(
            "CREATE TRIGGER fail_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT,'test failure'); END;",
        )
        .unwrap();
    assert!(
        b.queue_text(&bi, &contact.conversation_id, "must roll back".into(), 17)
            .is_err()
    );
    assert_eq!(
        b.messages(&contact.conversation_id, None, 50).unwrap().messages.len(),
        1
    );
}

#[test]
fn invitations_are_verified_and_cannot_add_self() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let (mut a, ai) = setup(a_dir.path(), "Алиса");
    let (b, _) = setup(b_dir.path(), "Борис");
    let invitation = a.create_invite(&ai, 10).unwrap();
    assert!(a.inspect_invite(&invitation, 11).is_err());
    assert!(b.inspect_invite(&invitation, i64::MAX).is_err());
    let mut bad = invitation.into_bytes();
    let last = bad.len() - 4;
    bad[last] = if bad[last] == b'A' { b'B' } else { b'A' };
    assert!(b.inspect_invite(std::str::from_utf8(&bad).unwrap(), 11).is_err());
}
