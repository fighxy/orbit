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
    assert_eq!(
        seen,
        vec![Event::ProfileChanged {
            profile: profile.clone()
        }]
    );
    match call(&engine, Command::GetSnapshot).unwrap() {
        CommandResult::Snapshot { profile: stored, .. } => assert_eq!(stored, Some(profile)),
        other => panic!("unexpected result {other:?}"),
    }
}
