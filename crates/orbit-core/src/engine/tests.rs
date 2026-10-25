use std::path::Path;

use super::*;
use crate::domain::{ConversationId, Message, MessageBody};
use crate::error::{ErrorCode, ErrorInfo};

fn open(dir: &Path, secret: &IdentitySecret) -> Engine {
    Engine::open(
        EngineConfig {
            data_dir: dir.to_path_buf(),
        },
        secret,
    )
    .unwrap()
}

/// Collects events until the result of `request_id` arrives.
fn result_of(
    engine: &Engine,
    request_id: RequestId,
    seen: &mut Vec<Event>,
) -> std::result::Result<CommandResult, ErrorInfo> {
    loop {
        let batch = engine.wait_events(Duration::from_secs(10)).unwrap();
        assert!(!batch.is_empty(), "timed out waiting for request {request_id}");
        for sequenced in batch {
            match sequenced.event {
                Event::CommandSucceeded { request_id: id, result } if id == request_id => return Ok(result),
                Event::CommandFailed { request_id: id, error } if id == request_id => return Err(error),
                other => seen.push(other),
            }
        }
    }
}

fn call(engine: &Engine, command: Command) -> std::result::Result<CommandResult, ErrorInfo> {
    let id = engine.submit(command).unwrap();
    result_of(engine, id, &mut Vec::new())
}

fn saved_messages(engine: &Engine) -> ConversationId {
    match call(engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot { conversations, .. } => conversations[0].id,
        other => panic!("unexpected result {other:?}"),
    }
}

fn send(engine: &Engine, conversation_id: ConversationId, text: &str) -> Message {
    match call(
        engine,
        Command::SendText {
            conversation_id,
            text: text.into(),
        },
    )
    .unwrap()
    {
        CommandResult::MessageSaved { message } => message,
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn snapshot_contains_identity_and_saved_messages() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let engine = open(dir.path(), &secret);
    match call(&engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot {
            identity,
            profile,
            conversations,
            ..
        } => {
            assert_eq!(profile, None);
            assert_eq!(&identity, engine.identity());
            identity.verify().unwrap();
            assert_eq!(conversations.len(), 1);
        }
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn sent_message_is_announced_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let conversation;
    let saved;
    {
        let engine = open(dir.path(), &secret);
        conversation = saved_messages(&engine);
        let id = engine
            .submit(Command::SendText {
                conversation_id: conversation,
                text: "  hello  ".into(),
            })
            .unwrap();
        let mut seen = Vec::new();
        saved = match result_of(&engine, id, &mut seen).unwrap() {
            CommandResult::MessageSaved { message } => message,
            other => panic!("unexpected result {other:?}"),
        };
        assert_eq!(saved.body, MessageBody::Text { text: "hello".into() });
        assert_eq!(seen, vec![Event::MessageAdded { message: saved.clone() }]);
        engine.close();
    }

    let engine = open(dir.path(), &secret);
    match call(
        &engine,
        Command::ListMessages {
            conversation_id: conversation,
            before_seq: None,
            limit: 10,
        },
    )
    .unwrap()
    {
        CommandResult::Messages { page } => assert_eq!(page.messages, vec![saved]),
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn a_thousand_messages_survive_restart_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let conversation;
    {
        let engine = open(dir.path(), &secret);
        conversation = saved_messages(&engine);
        for i in 0..1000 {
            send(&engine, conversation, &format!("message {i}"));
        }
    }
    let engine = open(dir.path(), &secret);
    let mut texts = Vec::new();
    let mut before_seq = None;
    loop {
        let page = match call(
            &engine,
            Command::ListMessages {
                conversation_id: conversation,
                before_seq,
                limit: 200,
            },
        )
        .unwrap()
        {
            CommandResult::Messages { page } => page,
            other => panic!("unexpected result {other:?}"),
        };
        before_seq = page.messages.first().map(|m| m.seq);
        let mut chunk: Vec<String> = page
            .messages
            .into_iter()
            .map(|m| match m.body {
                MessageBody::Text { text } => text,
                MessageBody::Deleted => String::new(),
                MessageBody::VoiceNote { .. } => String::new(),
            })
            .collect();
        chunk.append(&mut texts);
        texts = chunk;
        if !page.has_more {
            break;
        }
    }
    let expected: Vec<String> = (0..1000).map(|i| format!("message {i}")).collect();
    assert_eq!(texts, expected);
}

#[test]
fn invalid_commands_fail_with_typed_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path(), &IdentitySecret::generate().unwrap());
    let conversation = saved_messages(&engine);

    let empty = call(
        &engine,
        Command::SendText {
            conversation_id: conversation,
            text: "   ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(empty.code, ErrorCode::InvalidArgument);

    let unknown = call(
        &engine,
        Command::ListMessages {
            conversation_id: ConversationId::from_bytes([0; 16]),
            before_seq: None,
            limit: 10,
        },
    )
    .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound);
}

#[test]
fn relative_data_dir_is_rejected() {
    let error = Engine::open(
        EngineConfig {
            data_dir: "relative/dir".into(),
        },
        &IdentitySecret::generate().unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::InvalidArgument);
}

#[test]
fn second_engine_for_same_account_is_rejected_until_close() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    let first = open(dir.path(), &secret);
    let error = Engine::open(
        EngineConfig {
            data_dir: dir.path().to_path_buf(),
        },
        &secret,
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::StorageLocked);
    first.close();
    open(dir.path(), &secret);
}

#[test]
fn close_wakes_waiter_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(open(dir.path(), &IdentitySecret::generate().unwrap()));
    let waiter = {
        let engine = engine.clone();
        thread::spawn(move || engine.wait_events(Duration::from_secs(60)))
    };
    thread::sleep(Duration::from_millis(50));
    engine.close();
    assert!(matches!(waiter.join().unwrap(), Err(Error::Closed)));
    engine.close();
    assert!(engine.is_closed());
    assert!(matches!(engine.submit(Command::GetSnapshot), Err(Error::Closed)));
    assert!(matches!(engine.wait_events(Duration::ZERO), Err(Error::Closed)));
}

#[test]
fn concurrent_close_returns_after_lock_release() {
    let dir = tempfile::tempdir().unwrap();
    let secret = IdentitySecret::generate().unwrap();
    for _ in 0..20 {
        let engine = Arc::new(open(dir.path(), &secret));
        let conversation = saved_messages(&engine);
        for _ in 0..10 {
            engine
                .submit(Command::SendText {
                    conversation_id: conversation,
                    text: "x".into(),
                })
                .unwrap();
        }
        let closers: Vec<_> = (0..4)
            .map(|_| {
                let engine = engine.clone();
                thread::spawn(move || engine.close())
            })
            .collect();
        for closer in closers {
            closer.join().unwrap();
        }
        // Every close call has returned, so the storage lock must be free.
        drop(open(dir.path(), &secret));
    }
}

#[test]
fn cancel_wait_returns_empty_batch() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(open(dir.path(), &IdentitySecret::generate().unwrap()));
    let waiter = {
        let engine = engine.clone();
        thread::spawn(move || engine.wait_events(Duration::from_secs(60)))
    };
    thread::sleep(Duration::from_millis(50));
    engine.cancel_wait();
    assert!(waiter.join().unwrap().unwrap().is_empty());
}

#[test]
fn unread_results_bound_in_flight_commands() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path(), &IdentitySecret::generate().unwrap());
    for _ in 0..MAX_IN_FLIGHT_COMMANDS {
        engine.submit(Command::GetSnapshot).unwrap();
    }
    assert!(matches!(engine.submit(Command::GetSnapshot), Err(Error::Busy)));

    // Reading results frees capacity again.
    let mut results = 0;
    while results < MAX_IN_FLIGHT_COMMANDS {
        results += engine
            .wait_events(Duration::from_secs(10))
            .unwrap()
            .iter()
            .filter(|e| e.event.is_command_result())
            .count();
    }
    engine.submit(Command::GetSnapshot).unwrap();
}

#[test]
fn profile_update_is_validated_announced_and_in_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path(), &IdentitySecret::generate().unwrap());
    let invalid = call(
        &engine,
        Command::UpdateProfile {
            display_name: "  ".into(),
            about: String::new(),
        },
    )
    .unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidArgument);

    let id = engine
        .submit(Command::UpdateProfile {
            display_name: " Анна ".into(),
            about: "заметки".into(),
        })
        .unwrap();
    let mut seen = Vec::new();
    let profile = match result_of(&engine, id, &mut seen).unwrap() {
        CommandResult::ProfileUpdated { profile } => profile,
        other => panic!("unexpected result {other:?}"),
    };
    assert_eq!(profile.display_name, "Анна");
    assert!(matches!(seen.first(), Some(Event::ProfileChanged { profile: announced }) if announced == &profile));
    assert!(
        seen.iter()
            .skip(1)
            .all(|event| matches!(event, Event::NetworkChanged { .. }))
    );
    match call(&engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot { profile: stored, .. } => assert_eq!(stored, Some(profile)),
        other => panic!("unexpected result {other:?}"),
    }
}

fn network_of(engine: &Engine) -> crate::domain::NetworkStatus {
    match call(engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot { network, .. } => network,
        other => panic!("unexpected result {other:?}"),
    }
}

fn wait_online(engine: &Engine) {
    let start = std::time::Instant::now();
    loop {
        let network = network_of(engine);
        if network.state == crate::domain::ConnectionState::Online {
            return;
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("endpoint did not come online: {network:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

fn wait_ready(engine: &Engine) -> ConversationId {
    let start = std::time::Instant::now();
    loop {
        match call(engine, Command::GetSnapshot).unwrap() {
            CommandResult::Snapshot { conversations, .. } => {
                if let Some(conversation) = conversations.iter().find(|conversation| {
                    conversation.kind == crate::domain::ConversationKind::Direct
                        && conversation.contact.as_ref().is_some_and(|contact| contact.ready)
                }) {
                    return conversation.id;
                }
            }
            other => panic!("unexpected result {other:?}"),
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("contact exchange did not finish");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

fn texts_of(engine: &Engine, conversation: ConversationId) -> Vec<String> {
    match call(
        engine,
        Command::ListMessages {
            conversation_id: conversation,
            before_seq: None,
            limit: 20,
        },
    )
    .unwrap()
    {
        CommandResult::Messages { page } => page
            .messages
            .into_iter()
            .map(|message| match message.body {
                MessageBody::Text { text } => text,
                MessageBody::Deleted => String::new(),
                MessageBody::VoiceNote { .. } => String::new(),
            })
            .collect(),
        other => panic!("unexpected result {other:?}"),
    }
}

fn wait_text(engine: &Engine, conversation: ConversationId, text: &str) {
    let start = std::time::Instant::now();
    loop {
        if texts_of(engine, conversation).iter().any(|item| item == text) {
            return;
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("message {text:?} was not delivered");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

#[test]
fn two_accounts_exchange_text_by_invite_without_a_node() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a_secret = IdentitySecret::generate().unwrap();
    let b_secret = IdentitySecret::generate().unwrap();
    let a = open(a_dir.path(), &a_secret);
    let b = open(b_dir.path(), &b_secret);
    call(
        &a,
        Command::UpdateProfile {
            display_name: "Anya".into(),
            about: String::new(),
        },
    )
    .unwrap();
    call(
        &b,
        Command::UpdateProfile {
            display_name: "Borya".into(),
            about: String::new(),
        },
    )
    .unwrap();
    wait_online(&a);
    wait_online(&b);
    let invite = match call(&a, Command::CreateInvite).unwrap() {
        CommandResult::InviteCreated { text } => text,
        other => panic!("unexpected result {other:?}"),
    };
    let b_conversation = match call(&b, Command::AcceptInvite { text: invite }).unwrap() {
        CommandResult::ContactAdded { contact } => contact.conversation_id,
        other => panic!("unexpected result {other:?}"),
    };
    let a_conversation = wait_ready(&a);
    assert_eq!(wait_ready(&b), b_conversation);
    send(&b, b_conversation, "from borya");
    wait_text(&a, a_conversation, "from borya");
    send(&a, a_conversation, "from anya");
    wait_text(&b, b_conversation, "from anya");

    drop(a);
    let a = open(a_dir.path(), &a_secret);
    let texts = texts_of(&a, a_conversation);
    assert!(texts.iter().any(|text| text == "from borya"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "from anya"), "{texts:?}");
}

fn message_named(engine: &Engine, conversation: ConversationId, id: crate::domain::MessageId) -> Message {
    match call(
        engine,
        Command::ListMessages {
            conversation_id: conversation,
            before_seq: None,
            limit: 20,
        },
    )
    .unwrap()
    {
        CommandResult::Messages { page } => page.messages.into_iter().find(|message| message.id == id).unwrap(),
        other => panic!("unexpected result {other:?}"),
    }
}

fn wait_revision(
    engine: &Engine,
    conversation: ConversationId,
    id: crate::domain::MessageId,
    revision: u32,
) -> Message {
    let start = std::time::Instant::now();
    loop {
        let message = message_named(engine, conversation, id);
        if message.revision >= revision {
            return message;
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("revision {revision} was not delivered: {message:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

#[test]
fn author_edit_and_delete_reach_the_contact() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a_secret = IdentitySecret::generate().unwrap();
    let b_secret = IdentitySecret::generate().unwrap();
    let a = open(a_dir.path(), &a_secret);
    let b = open(b_dir.path(), &b_secret);
    let a_saved = saved_messages(&a);
    call(
        &a,
        Command::UpdateProfile {
            display_name: "Anya".into(),
            about: String::new(),
        },
    )
    .unwrap();
    call(
        &b,
        Command::UpdateProfile {
            display_name: "Borya".into(),
            about: String::new(),
        },
    )
    .unwrap();
    wait_online(&a);
    wait_online(&b);
    let invite = match call(&a, Command::CreateInvite).unwrap() {
        CommandResult::InviteCreated { text } => text,
        other => panic!("unexpected result {other:?}"),
    };
    let b_conversation = match call(&b, Command::AcceptInvite { text: invite }).unwrap() {
        CommandResult::ContactAdded { contact } => contact.conversation_id,
        other => panic!("unexpected result {other:?}"),
    };
    let a_conversation = wait_ready(&a);
    assert_eq!(wait_ready(&b), b_conversation);
    let sent = send(&b, b_conversation, "original");
    wait_text(&a, a_conversation, "original");
    call(
        &b,
        Command::EditText {
            conversation_id: b_conversation,
            message_id: sent.id,
            text: "changed".into(),
        },
    )
    .unwrap();
    let edited = wait_revision(&a, a_conversation, sent.id, 1);
    assert_eq!(edited.body, MessageBody::Text { text: "changed".into() });
    assert!(edited.edited_at_ms.is_some());
    call(
        &b,
        Command::DeleteText {
            conversation_id: b_conversation,
            message_id: sent.id,
        },
    )
    .unwrap();
    let deleted = wait_revision(&a, a_conversation, sent.id, 2);
    assert!(deleted.deleted);
    assert_eq!(deleted.body, MessageBody::Deleted);
    let forbidden = call(
        &a,
        Command::EditText {
            conversation_id: a_conversation,
            message_id: sent.id,
            text: "nope".into(),
        },
    );
    assert_eq!(forbidden.unwrap_err().code, ErrorCode::InvalidArgument);

    let note = send(&a, a_saved, "note");
    let local = match call(
        &a,
        Command::EditText {
            conversation_id: a_saved,
            message_id: note.id,
            text: "note 2".into(),
        },
    )
    .unwrap()
    {
        CommandResult::MessageSaved { message } => message,
        other => panic!("unexpected result {other:?}"),
    };
    assert_eq!(local.revision, 1);
    assert_eq!(local.body, MessageBody::Text { text: "note 2".into() });
    assert_eq!(message_named(&a, a_saved, note.id).body, local.body);
}

fn ready_direct_ids(engine: &Engine) -> Vec<ConversationId> {
    match call(engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot { conversations, .. } => conversations
            .into_iter()
            .filter(|conversation| {
                conversation.kind == ConversationKind::Direct
                    && conversation.contact.as_ref().is_some_and(|contact| contact.ready)
            })
            .map(|conversation| conversation.id)
            .collect(),
        other => panic!("unexpected result {other:?}"),
    }
}

/// Invite from `a` to `b`. Returns the direct conversation id on each side.
fn connect(a: &Engine, b: &Engine) -> (ConversationId, ConversationId) {
    let before: std::collections::HashSet<_> = ready_direct_ids(a).into_iter().collect();
    let invite = match call(a, Command::CreateInvite).unwrap() {
        CommandResult::InviteCreated { text } => text,
        other => panic!("unexpected result {other:?}"),
    };
    let b_conversation = match call(b, Command::AcceptInvite { text: invite }).unwrap() {
        CommandResult::ContactAdded { contact } => contact.conversation_id,
        other => panic!("unexpected result {other:?}"),
    };
    let start = Instant::now();
    let a_conversation = loop {
        if let Some(id) = ready_direct_ids(a).into_iter().find(|id| !before.contains(id)) {
            break id;
        }
        if start.elapsed() > Duration::from_secs(25) {
            panic!("contact exchange did not finish");
        }
        thread::sleep(Duration::from_millis(40));
    };
    let start = Instant::now();
    loop {
        if ready_direct_ids(b).contains(&b_conversation) {
            break;
        }
        if start.elapsed() > Duration::from_secs(25) {
            panic!("contact exchange did not finish");
        }
        thread::sleep(Duration::from_millis(40));
    }
    (a_conversation, b_conversation)
}

fn wait_room(engine: &Engine, title: &str) -> crate::domain::Conversation {
    let start = Instant::now();
    loop {
        match call(engine, Command::GetSnapshot).unwrap() {
            CommandResult::Snapshot { conversations, .. } => {
                if let Some(conversation) = conversations
                    .into_iter()
                    .find(|item| item.title.as_deref() == Some(title))
                {
                    return conversation;
                }
            }
            other => panic!("unexpected result {other:?}"),
        }
        if start.elapsed() > Duration::from_secs(25) {
            panic!("room {title} was not delivered");
        }
        thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn three_accounts_share_a_group_and_a_channel() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let c_dir = tempfile::tempdir().unwrap();
    let a_secret = IdentitySecret::generate().unwrap();
    let b_secret = IdentitySecret::generate().unwrap();
    let c_secret = IdentitySecret::generate().unwrap();
    let a = open(a_dir.path(), &a_secret);
    let b = open(b_dir.path(), &b_secret);
    let c = open(c_dir.path(), &c_secret);
    for (engine, name) in [(&a, "Anya"), (&b, "Borya"), (&c, "Vera")] {
        call(
            engine,
            Command::UpdateProfile {
                display_name: name.into(),
                about: String::new(),
            },
        )
        .unwrap();
    }
    wait_online(&a);
    wait_online(&b);
    wait_online(&c);
    let (a_b, _) = connect(&a, &b);
    let (a_c, _) = connect(&a, &c);

    let group = match call(
        &a,
        Command::CreateGroup {
            title: "кухня".into(),
            members: vec![a_b, a_c],
        },
    )
    .unwrap()
    {
        CommandResult::RoomCreated { conversation } => conversation,
        other => panic!("unexpected result {other:?}"),
    };
    assert_eq!(group.kind, ConversationKind::Group);
    assert_eq!(group.title.as_deref(), Some("кухня"));
    assert!(group.can_post);
    let on_b = wait_room(&b, "кухня");
    let on_c = wait_room(&c, "кухня");
    assert_eq!(on_b.id, group.id);
    assert_eq!(on_c.id, group.id);
    assert!(on_b.can_post && on_c.can_post);

    let sent = send(&a, group.id, "суп");
    wait_text(&b, group.id, "суп");
    wait_text(&c, group.id, "суп");
    send(&b, group.id, "борщ");
    wait_text(&a, group.id, "борщ");
    wait_text(&c, group.id, "борщ");
    call(
        &a,
        Command::EditText {
            conversation_id: group.id,
            message_id: sent.id,
            text: "суп 2".into(),
        },
    )
    .unwrap();
    let edited = wait_revision(&b, group.id, sent.id, 1);
    assert_eq!(
        edited.body,
        MessageBody::Text {
            text: "суп 2".into()
        }
    );
    call(
        &a,
        Command::DeleteText {
            conversation_id: group.id,
            message_id: sent.id,
        },
    )
    .unwrap();
    let deleted = wait_revision(&b, group.id, sent.id, 2);
    assert!(deleted.deleted);
    assert_eq!(deleted.body, MessageBody::Deleted);
    assert_eq!(wait_revision(&c, group.id, sent.id, 2).body, MessageBody::Deleted);

    drop(b);
    let b = open(b_dir.path(), &b_secret);
    wait_online(&b);
    let restored = texts_of(&b, group.id);
    assert!(restored.iter().any(|text| text.is_empty()), "{restored:?}");
    assert!(restored.iter().any(|text| text == "борщ"), "{restored:?}");

    let channel = match call(
        &a,
        Command::CreateChannel {
            title: "новости".into(),
            members: vec![a_b],
        },
    )
    .unwrap()
    {
        CommandResult::RoomCreated { conversation } => conversation,
        other => panic!("unexpected result {other:?}"),
    };
    assert!(channel.can_post);
    let on_b = wait_room(&b, "новости");
    assert_eq!(on_b.id, channel.id);
    assert!(!on_b.can_post);
    send(&a, channel.id, "новость");
    wait_text(&b, channel.id, "новость");
    let forbidden = call(
        &b,
        Command::SendText {
            conversation_id: channel.id,
            text: "нет".into(),
        },
    );
    assert_eq!(forbidden.unwrap_err().code, ErrorCode::InvalidArgument);
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(text: String) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(text).unwrap()
}

fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00,
        0x05, 0xFE, 0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn exchange(a: &Engine, b: &Engine) -> (ConversationId, ConversationId) {
    wait_online(a);
    wait_online(b);
    let invite = match call(a, Command::CreateInvite).unwrap() {
        CommandResult::InviteCreated { text } => text,
        other => panic!("unexpected result {other:?}"),
    };
    let b_conversation = match call(b, Command::AcceptInvite { text: invite }).unwrap() {
        CommandResult::ContactAdded { contact } => contact.conversation_id,
        other => panic!("unexpected result {other:?}"),
    };
    let a_conversation = wait_ready(a);
    assert_eq!(wait_ready(b), b_conversation);
    (a_conversation, b_conversation)
}

fn wait_avatar(engine: &Engine, conversation: ConversationId, expected: &[u8]) {
    let start = std::time::Instant::now();
    loop {
        match call(engine, Command::GetSnapshot).unwrap() {
            CommandResult::Snapshot { conversations, .. } => {
                let avatar = conversations
                    .iter()
                    .find(|item| item.id == conversation)
                    .and_then(|item| item.contact.as_ref())
                    .and_then(|contact| contact.avatar.as_deref());
                if avatar == Some(expected) {
                    return;
                }
            }
            other => panic!("unexpected result {other:?}"),
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("avatar was not delivered");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

#[test]
fn avatar_reaches_the_direct_contact_and_a_later_one_replaces_it() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = open(a_dir.path(), &IdentitySecret::generate().unwrap());
    let b = open(b_dir.path(), &IdentitySecret::generate().unwrap());
    call(
        &a,
        Command::UpdateProfile {
            display_name: "anya1".into(),
            about: String::new(),
        },
    )
    .unwrap();
    call(
        &b,
        Command::UpdateProfile {
            display_name: "borya2".into(),
            about: String::new(),
        },
    )
    .unwrap();
    let picture = tiny_png();
    match call(&a, Command::SetAvatar { image: b64(&picture) }).unwrap() {
        CommandResult::ProfileUpdated { profile } => assert_eq!(profile.avatar.as_deref(), Some(picture.as_slice())),
        other => panic!("unexpected result {other:?}"),
    }
    assert_eq!(
        call(
            &a,
            Command::SetAvatar {
                image: b64(b"not an image"),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArgument
    );
    let (_a_conversation, b_conversation) = exchange(&a, &b);
    wait_avatar(&b, b_conversation, &picture);
    let mut replacement = picture.clone();
    replacement.push(0);
    call(
        &a,
        Command::SetAvatar {
            image: b64(&replacement),
        },
    )
    .unwrap();
    wait_avatar(&b, b_conversation, &replacement);
}

#[test]
fn voice_note_round_trips_locally_and_reaches_the_contact() {
    let wav = crate::storage::sample_voice_wav(1_200);
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path(), &IdentitySecret::generate().unwrap());
    let saved = saved_messages(&engine);
    let stored = match call(
        &engine,
        Command::SendVoice {
            conversation_id: saved,
            wav_base64: b64(&wav),
        },
    )
    .unwrap()
    {
        CommandResult::MessageSaved { message } => message,
        other => panic!("unexpected result {other:?}"),
    };
    assert!(matches!(stored.body, MessageBody::VoiceNote { duration_ms: 1_200, .. }));
    match call(&engine, Command::ReadVoice { message_id: stored.id }).unwrap() {
        CommandResult::Voice { wav_base64, .. } => assert_eq!(unb64(wav_base64), wav),
        other => panic!("unexpected result {other:?}"),
    }
    assert_eq!(
        call(
            &engine,
            Command::EditText {
                conversation_id: saved,
                message_id: stored.id,
                text: "нет".into(),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArgument
    );
    call(
        &engine,
        Command::DeleteText {
            conversation_id: saved,
            message_id: stored.id,
        },
    )
    .unwrap();
    assert_eq!(
        call(&engine, Command::ReadVoice { message_id: stored.id })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        call(
            &engine,
            Command::SendVoice {
                conversation_id: saved,
                wav_base64: b64(b"not a wav"),
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArgument
    );

    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = open(a_dir.path(), &IdentitySecret::generate().unwrap());
    let b = open(b_dir.path(), &IdentitySecret::generate().unwrap());
    call(
        &a,
        Command::UpdateProfile {
            display_name: "anya1".into(),
            about: String::new(),
        },
    )
    .unwrap();
    call(
        &b,
        Command::UpdateProfile {
            display_name: "borya2".into(),
            about: String::new(),
        },
    )
    .unwrap();
    let (a_conversation, b_conversation) = exchange(&a, &b);
    let sent = match call(
        &a,
        Command::SendVoice {
            conversation_id: a_conversation,
            wav_base64: b64(&wav),
        },
    )
    .unwrap()
    {
        CommandResult::MessageSaved { message } => message,
        other => panic!("unexpected result {other:?}"),
    };
    let start = std::time::Instant::now();
    let received = loop {
        match call(
            &b,
            Command::ListMessages {
                conversation_id: b_conversation,
                before_seq: None,
                limit: 10,
            },
        )
        .unwrap()
        {
            CommandResult::Messages { page } => {
                if let Some(message) = page
                    .messages
                    .into_iter()
                    .find(|message| matches!(message.body, MessageBody::VoiceNote { duration_ms: 1_200, .. }))
                {
                    break message;
                }
            }
            other => panic!("unexpected result {other:?}"),
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("voice note was not delivered");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    match call(
        &b,
        Command::ReadVoice {
            message_id: received.id,
        },
    )
    .unwrap()
    {
        CommandResult::Voice { wav_base64, .. } => assert_eq!(unb64(wav_base64), wav),
        other => panic!("unexpected result {other:?}"),
    }
    let start = std::time::Instant::now();
    loop {
        match call(
            &a,
            Command::ListMessages {
                conversation_id: a_conversation,
                before_seq: None,
                limit: 10,
            },
        )
        .unwrap()
        {
            CommandResult::Messages { page } => {
                if page
                    .messages
                    .iter()
                    .any(|message| message.id == sent.id && message.state == crate::domain::MessageState::Delivered)
                {
                    break;
                }
            }
            other => panic!("unexpected result {other:?}"),
        }
        if start.elapsed() > std::time::Duration::from_secs(25) {
            panic!("voice note was not acknowledged");
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}
