//! Voice notes: a PCM WAV split into envelopes that fit the 64 KiB cap.
//!
//! Telegram records Opus and draws a waveform. This slice uses 16 kHz mono
//! 16-bit PCM so a note can be checked without an Opus codec. It is not a
//! video circle and not a live voice room.

use orbit_protocol::envelope::{
    DeviceIdentity, MAX_VOICE_BYTES, MAX_VOICE_CHUNK_BYTES, MAX_VOICE_MS, MAX_WAVEFORM_BARS, Payload,
};
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use super::Store;
use super::cipher::SealedBody;
use super::delivery::{self, seal};
use crate::domain::{AccountId, ConversationId, ConversationKind, DeviceId, MessageBody, MessageId, MessageState};
use crate::error::{Error, Result};
use crate::identity::LocalIdentity;

const SAMPLE_RATE: u32 = 16_000;
const MAX_INCOMPLETE: i64 = 8;
const MAX_HELD_CHUNKS: i64 = 512;

pub(super) struct VoiceEffect {
    pub id: MessageId,
    pub meta: Option<VoiceMeta>,
    pub chunk: Option<(MessageId, u16, Vec<u8>)>,
    pub finish: Option<VoiceFinish>,
    pub receipt: bool,
}

pub(super) struct VoiceMeta {
    pub id: MessageId,
    pub conversation: ConversationId,
    pub author_account: AccountId,
    pub author_device: DeviceId,
    pub sent_at_ms: i64,
    pub duration_ms: u32,
    pub byte_len: u32,
    pub sha256: [u8; 32],
    pub chunk_count: u16,
    pub waveform: Vec<u8>,
}

pub(super) struct VoiceFinish {
    pub id: MessageId,
    pub sealed: SealedBody,
    pub wav: Vec<u8>,
}

pub(super) struct VoiceStart {
    pub message_id: [u8; 16],
    pub duration_ms: u32,
    pub byte_len: u32,
    pub sha256: [u8; 32],
    pub chunk_count: u16,
    pub waveform: Vec<u8>,
}

struct ParsedWav {
    duration_ms: u32,
    waveform: Vec<u8>,
}

struct HeldMeta {
    conversation: ConversationId,
    author_account: AccountId,
    author_device: DeviceId,
    sent_at_ms: i64,
    duration_ms: u32,
    byte_len: u32,
    sha256: [u8; 32],
    chunk_count: u16,
    waveform: Vec<u8>,
}

/// Builds a deterministic PCM WAV for tests. A square wave, not a recording.
#[cfg(test)]
pub(crate) fn sample_voice_wav(duration_ms: u32) -> Vec<u8> {
    assert!((100..=MAX_VOICE_MS).contains(&duration_ms));
    let count = usize::try_from(u64::from(duration_ms) * u64::from(SAMPLE_RATE) / 1000).unwrap();
    let samples: Vec<i16> = (0..count)
        .map(|index| if (index / 16).is_multiple_of(2) { 8000 } else { -8000 })
        .collect();
    encode_wav(&samples)
}

#[cfg(test)]
fn encode_wav(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&u32::try_from(36 + data_len).unwrap().to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&u32::try_from(data_len).unwrap().to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn parse_voice_wav(bytes: &[u8]) -> Result<ParsedWav> {
    if bytes.len() < 44 || bytes.len() > MAX_VOICE_BYTES || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::InvalidArgument("voice must be a PCM WAV".into()));
    }
    let mut format = None;
    let mut data = None;
    let mut cursor = 12;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::InvalidArgument("voice WAV".into()))?;
        if end > bytes.len() {
            return Err(Error::InvalidArgument("voice WAV".into()));
        }
        match id {
            b"fmt " => format = Some(&bytes[start..end]),
            b"data" => data = Some(&bytes[start..end]),
            _ => return Err(Error::InvalidArgument("voice WAV has an unexpected chunk".into())),
        }
        cursor = end + (size % 2);
    }
    if cursor != bytes.len() {
        return Err(Error::InvalidArgument("voice WAV".into()));
    }
    let format = format.ok_or_else(|| Error::InvalidArgument("voice WAV has no format".into()))?;
    let data = data.ok_or_else(|| Error::InvalidArgument("voice WAV has no audio".into()))?;
    if format.len() < 16 {
        return Err(Error::InvalidArgument("voice WAV".into()));
    }
    let audio_format = u16::from_le_bytes(format[0..2].try_into().unwrap());
    let channels = u16::from_le_bytes(format[2..4].try_into().unwrap());
    let rate = u32::from_le_bytes(format[4..8].try_into().unwrap());
    let bits = u16::from_le_bytes(format[14..16].try_into().unwrap());
    if audio_format != 1 || channels != 1 || rate != SAMPLE_RATE || bits != 16 || !data.len().is_multiple_of(2) {
        return Err(Error::InvalidArgument("voice must be 16 kHz mono 16-bit PCM".into()));
    }
    let samples = data.len() / 2;
    let duration_ms = u32::try_from(samples as u64 * 1000 / u64::from(SAMPLE_RATE)).unwrap_or(u32::MAX);
    if duration_ms == 0 || duration_ms > MAX_VOICE_MS {
        return Err(Error::InvalidArgument("voice duration".into()));
    }
    Ok(ParsedWav {
        duration_ms,
        waveform: waveform(data),
    })
}

fn waveform(data: &[u8]) -> Vec<u8> {
    let samples = data.len() / 2;
    let bars = MAX_WAVEFORM_BARS.min(samples.max(1));
    let mut out = Vec::with_capacity(bars);
    for bar in 0..bars {
        let start = bar * samples / bars;
        let end = ((bar + 1) * samples / bars).max(start + 1);
        let mut peak = 0u16;
        for index in start..end {
            let sample = i16::from_le_bytes([data[index * 2], data[index * 2 + 1]]).unsigned_abs();
            peak = peak.max(sample);
        }
        out.push(u8::try_from(u32::from(peak) * 255 / 32768).unwrap_or(255));
    }
    out
}

impl Store {
    pub(crate) fn send_voice(
        &mut self,
        identity: &LocalIdentity,
        conversation: &ConversationId,
        wav: &[u8],
        now: i64,
    ) -> Result<crate::domain::Message> {
        let parsed = parse_voice_wav(wav)?;
        let kind = self.conversation(conversation)?.kind;
        if !matches!(kind, ConversationKind::SavedMessages | ConversationKind::Direct) {
            return Err(Error::InvalidArgument(
                "voice notes are only in direct chats and saved messages".into(),
            ));
        }
        let id = MessageId::random()?;
        let body = MessageBody::VoiceNote {
            duration_ms: parsed.duration_ms,
            waveform: parsed.waveform.clone(),
        };
        let sealed = delivery::seal_body(self, &id, conversation, &body)?;
        let (state, jobs) = if kind == ConversationKind::SavedMessages {
            (MessageState::SavedLocally, Vec::new())
        } else {
            let (card, ready) = self.contact_card(conversation)?.ok_or(Error::NotFound("contact"))?;
            if !ready {
                return Err(Error::InvalidArgument("contact is not ready".into()));
            }
            let hash = Sha256::digest(wav);
            let mut sha = [0u8; 32];
            sha.copy_from_slice(&hash);
            let chunks: Vec<&[u8]> = wav.chunks(MAX_VOICE_CHUNK_BYTES).collect();
            let mut jobs = Vec::with_capacity(chunks.len() + 1);
            let start = Payload::MediaStart {
                message_id: *id.as_bytes(),
                duration_ms: parsed.duration_ms,
                byte_len: u32::try_from(wav.len()).map_err(|_| Error::InvalidArgument("voice is too large".into()))?,
                sha256: sha,
                chunk_count: u16::try_from(chunks.len())
                    .map_err(|_| Error::InvalidArgument("voice is too large".into()))?,
                waveform: parsed.waveform,
            };
            jobs.push(sealed_job(self, identity, &card, now, &start)?);
            for (index, bytes) in chunks.iter().enumerate() {
                let chunk = Payload::MediaChunk {
                    message_id: *id.as_bytes(),
                    index: u16::try_from(index).unwrap(),
                    bytes: bytes.to_vec(),
                };
                jobs.push(sealed_job(self, identity, &card, now, &chunk)?);
            }
            (MessageState::Queued, jobs)
        };
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        delivery::insert_message(
            &tx,
            &id,
            conversation,
            &self.account_id,
            &self.device_id,
            now,
            &sealed,
            state,
        )?;
        let seq = u64::try_from(tx.last_insert_rowid()).map_err(|_| Error::Corrupted("negative message seq"))?;
        store_wav(&tx, &id, wav)?;
        for (job, envelope, route) in &jobs {
            delivery::insert_outbox(&tx, job, conversation, Some(&id), envelope, route)?;
        }
        tx.commit()?;
        Ok(crate::domain::Message {
            id,
            conversation_id: *conversation,
            seq,
            author_account: self.account_id,
            author_device: self.device_id,
            created_at_ms: now,
            body,
            state,
            revision: 0,
            edited_at_ms: None,
            deleted: false,
        })
    }

    pub(crate) fn read_voice(&self, id: &MessageId) -> Result<Vec<u8>> {
        self.conn
            .query_row(
                "SELECT wav FROM voice_notes WHERE message_id=?1",
                [id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound("voice"))
    }

    pub(super) fn plan_voice_start(
        &self,
        sender: &DeviceIdentity,
        conversation: &ConversationId,
        sent_at_ms: i64,
        start: VoiceStart,
    ) -> Result<VoiceEffect> {
        self.pinned_sender(conversation, sender)?;
        if start.waveform.len() > MAX_WAVEFORM_BARS {
            return Err(Error::InvalidArgument("voice waveform".into()));
        }
        let id = MessageId::from_bytes(start.message_id);
        if self.known_voice(&id, conversation, sender)? {
            return Ok(receipt_only(id));
        }
        let meta = VoiceMeta {
            id,
            conversation: *conversation,
            author_account: AccountId::from_bytes(sender.account),
            author_device: DeviceId::from_bytes(sender.device),
            sent_at_ms,
            duration_ms: start.duration_ms,
            byte_len: start.byte_len,
            sha256: start.sha256,
            chunk_count: start.chunk_count,
            waveform: start.waveform,
        };
        if let Some(held) = self.held_meta(&id)? {
            if !same_meta(&held, &meta) {
                return Err(Error::InvalidArgument("conflicting voice note".into()));
            }
            return self.finish_from_stored(&meta, None);
        }
        self.ensure_room_for_meta()?;
        let mut effect = self.finish_from_stored(&meta, None)?;
        effect.meta = Some(meta);
        Ok(effect)
    }

    pub(super) fn plan_voice_chunk(
        &self,
        sender: &DeviceIdentity,
        conversation: &ConversationId,
        message_id: [u8; 16],
        index: u16,
        bytes: Vec<u8>,
    ) -> Result<VoiceEffect> {
        self.pinned_sender(conversation, sender)?;
        if bytes.is_empty() || bytes.len() > MAX_VOICE_CHUNK_BYTES {
            return Err(Error::InvalidArgument("voice slice".into()));
        }
        let id = MessageId::from_bytes(message_id);
        if self.known_voice(&id, conversation, sender)? {
            return Ok(receipt_only(id));
        }
        if let Some(existing) = self.chunk_bytes(&id, index)? {
            if existing != bytes {
                return Err(Error::InvalidArgument("conflicting voice slice".into()));
            }
            let Some(meta) = self.held_meta(&id)? else {
                return Ok(empty_effect(id));
            };
            return self.finish_from_stored(&meta_from_held(&id, meta), None);
        }
        self.ensure_room_for_chunk()?;
        let Some(held) = self.held_meta(&id)? else {
            return Ok(VoiceEffect {
                id,
                meta: None,
                chunk: Some((id, index, bytes)),
                finish: None,
                receipt: false,
            });
        };
        if index >= held.chunk_count {
            return Err(Error::InvalidArgument("voice slice".into()));
        }
        let meta = meta_from_held(&id, held);
        let mut effect = self.finish_from_stored(&meta, Some((index, &bytes)))?;
        effect.chunk = Some((id, index, bytes));
        Ok(effect)
    }

    fn pinned_sender(&self, conversation: &ConversationId, sender: &DeviceIdentity) -> Result<()> {
        let (card, _) = self.contact_card(conversation)?.ok_or(Error::NotFound("contact"))?;
        if card.identity != *sender {
            return Err(Error::InvalidArgument("sender is not the pinned contact".into()));
        }
        Ok(())
    }

    fn known_voice(&self, id: &MessageId, conversation: &ConversationId, sender: &DeviceIdentity) -> Result<bool> {
        let Some(old) = self.lookup_message(id)? else {
            return Ok(false);
        };
        let same = old.conversation_id == *conversation
            && old.author_device.as_bytes() == &sender.device
            && old.author_account.as_bytes() == &sender.account
            && matches!(old.body, MessageBody::VoiceNote { .. } | MessageBody::Deleted);
        if !same {
            return Err(Error::InvalidArgument("conflicting message id".into()));
        }
        Ok(true)
    }

    fn held_meta(&self, id: &MessageId) -> Result<Option<HeldMeta>> {
        self.conn
            .query_row(
                "SELECT conversation_id, author_account, author_device, sent_at_ms, duration_ms, byte_len, \
                        sha256, chunk_count, waveform FROM voice_incoming_meta WHERE message_id=?1",
                [id.as_bytes().as_slice()],
                |row| {
                    let conversation = row.get::<_, Vec<u8>>(0)?;
                    let account = row.get::<_, Vec<u8>>(1)?;
                    let device = row.get::<_, Vec<u8>>(2)?;
                    let sha = row.get::<_, Vec<u8>>(6)?;
                    Ok((
                        conversation,
                        account,
                        device,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        sha,
                        row.get::<_, i64>(7)?,
                        row.get::<_, Vec<u8>>(8)?,
                    ))
                },
            )
            .optional()?
            .map(
                |(conversation, account, device, sent_at_ms, duration_ms, byte_len, sha, chunk_count, waveform)| {
                    let conversation = ConversationId::from_slice(&conversation)
                        .map_err(|_| Error::Corrupted("voice conversation"))?;
                    let author_account =
                        AccountId::from_slice(&account).map_err(|_| Error::Corrupted("voice author"))?;
                    let author_device = DeviceId::from_slice(&device).map_err(|_| Error::Corrupted("voice device"))?;
                    let sha256: [u8; 32] = sha.try_into().map_err(|_| Error::Corrupted("voice hash"))?;
                    Ok(HeldMeta {
                        conversation,
                        author_account,
                        author_device,
                        sent_at_ms,
                        duration_ms: u32::try_from(duration_ms).unwrap_or(0),
                        byte_len: u32::try_from(byte_len).unwrap_or(0),
                        sha256,
                        chunk_count: u16::try_from(chunk_count).unwrap_or(0),
                        waveform,
                    })
                },
            )
            .transpose()
    }

    fn chunk_bytes(&self, id: &MessageId, index: u16) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT bytes FROM voice_incoming WHERE message_id=?1 AND chunk_index=?2",
                params![id.as_bytes().as_slice(), i64::from(index)],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)
    }

    fn ensure_room_for_meta(&self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM voice_incoming_meta", [], |row| row.get(0))?;
        if count >= MAX_INCOMPLETE {
            return Err(Error::Busy);
        }
        Ok(())
    }

    fn ensure_room_for_chunk(&self) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM voice_incoming", [], |row| row.get(0))?;
        if count >= MAX_HELD_CHUNKS {
            return Err(Error::Busy);
        }
        Ok(())
    }

    fn finish_from_stored(&self, meta: &VoiceMeta, extra: Option<(u16, &[u8])>) -> Result<VoiceEffect> {
        let mut chunks = self.chunks_of(&meta.id)?;
        if let Some((index, bytes)) = extra {
            chunks.retain(|(stored, _)| *stored != index);
            chunks.push((index, bytes.to_vec()));
            chunks.sort_by_key(|(index, _)| *index);
        }
        if chunks
            .iter()
            .any(|(index, _)| usize::from(*index) >= usize::from(meta.chunk_count))
        {
            return Err(Error::InvalidArgument("voice slice".into()));
        }
        if chunks.len() != usize::from(meta.chunk_count) {
            return Ok(empty_effect(meta.id));
        }
        let mut wav = Vec::with_capacity(meta.byte_len as usize);
        for (position, (index, bytes)) in chunks.iter().enumerate() {
            if *index != u16::try_from(position).unwrap_or(u16::MAX) {
                return Err(Error::InvalidArgument("voice slices are incomplete".into()));
            }
            wav.extend_from_slice(bytes);
        }
        if wav.len() != meta.byte_len as usize {
            return Err(Error::InvalidArgument("voice length".into()));
        }
        let hash = Sha256::digest(&wav);
        if hash.as_slice() != meta.sha256 {
            return Err(Error::InvalidArgument("voice hash".into()));
        }
        let parsed = parse_voice_wav(&wav)?;
        if parsed.duration_ms != meta.duration_ms {
            return Err(Error::InvalidArgument("voice duration".into()));
        }
        let body = MessageBody::VoiceNote {
            duration_ms: parsed.duration_ms,
            waveform: parsed.waveform,
        };
        let sealed = delivery::seal_body(self, &meta.id, &meta.conversation, &body)?;
        Ok(VoiceEffect {
            id: meta.id,
            meta: None,
            chunk: None,
            finish: Some(VoiceFinish {
                id: meta.id,
                sealed,
                wav,
            }),
            receipt: true,
        })
    }

    fn chunks_of(&self, id: &MessageId) -> Result<Vec<(u16, Vec<u8>)>> {
        let mut statement = self
            .conn
            .prepare("SELECT chunk_index, bytes FROM voice_incoming WHERE message_id=?1 ORDER BY chunk_index")?;
        let rows = statement.query_map([id.as_bytes().as_slice()], |row| {
            Ok((u16::try_from(row.get::<_, i64>(0)?).unwrap_or(u16::MAX), row.get(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Error::from)
    }
}

fn sealed_job(
    store: &Store,
    identity: &LocalIdentity,
    card: &orbit_protocol::envelope::ContactCard,
    now: i64,
    payload: &Payload,
) -> Result<(MessageId, Vec<u8>, SealedBody)> {
    let job = MessageId::random()?;
    let envelope = seal(identity, card, now, payload)?;
    let route = store.seal_record("route", job.as_bytes(), card)?;
    Ok((job, envelope, route))
}

fn same_meta(held: &HeldMeta, meta: &VoiceMeta) -> bool {
    held.conversation == meta.conversation
        && held.author_account == meta.author_account
        && held.author_device == meta.author_device
        && held.duration_ms == meta.duration_ms
        && held.byte_len == meta.byte_len
        && held.sha256 == meta.sha256
        && held.chunk_count == meta.chunk_count
}

fn meta_from_held(id: &MessageId, held: HeldMeta) -> VoiceMeta {
    VoiceMeta {
        id: *id,
        conversation: held.conversation,
        author_account: held.author_account,
        author_device: held.author_device,
        sent_at_ms: held.sent_at_ms,
        duration_ms: held.duration_ms,
        byte_len: held.byte_len,
        sha256: held.sha256,
        chunk_count: held.chunk_count,
        waveform: held.waveform,
    }
}

fn receipt_only(id: MessageId) -> VoiceEffect {
    VoiceEffect {
        id,
        meta: None,
        chunk: None,
        finish: None,
        receipt: true,
    }
}

fn empty_effect(id: MessageId) -> VoiceEffect {
    VoiceEffect {
        id,
        meta: None,
        chunk: None,
        finish: None,
        receipt: false,
    }
}

pub(super) fn write_effect(tx: &Transaction<'_>, effect: &VoiceEffect) -> Result<()> {
    if let Some(meta) = &effect.meta {
        tx.execute(
            "INSERT INTO voice_incoming_meta (message_id, conversation_id, author_account, author_device, \
                 sent_at_ms, duration_ms, byte_len, sha256, chunk_count, waveform) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                meta.id.as_bytes().as_slice(),
                meta.conversation.as_bytes().as_slice(),
                meta.author_account.as_bytes().as_slice(),
                meta.author_device.as_bytes().as_slice(),
                meta.sent_at_ms,
                i64::from(meta.duration_ms),
                i64::from(meta.byte_len),
                meta.sha256.as_slice(),
                i64::from(meta.chunk_count),
                meta.waveform,
            ],
        )?;
    }
    if let Some((id, index, bytes)) = &effect.chunk {
        tx.execute(
            "INSERT INTO voice_incoming (message_id, chunk_index, bytes) VALUES (?1,?2,?3)",
            params![id.as_bytes().as_slice(), i64::from(*index), bytes],
        )?;
    }
    Ok(())
}

pub(super) fn store_wav(tx: &Transaction<'_>, id: &MessageId, wav: &[u8]) -> Result<()> {
    tx.execute(
        "INSERT INTO voice_notes (message_id, wav) VALUES (?1,?2)",
        params![id.as_bytes().as_slice(), wav],
    )?;
    Ok(())
}

pub(super) fn clear_incoming(tx: &Transaction<'_>, id: &MessageId) -> Result<()> {
    tx.execute(
        "DELETE FROM voice_incoming WHERE message_id=?1",
        [id.as_bytes().as_slice()],
    )?;
    tx.execute(
        "DELETE FROM voice_incoming_meta WHERE message_id=?1",
        [id.as_bytes().as_slice()],
    )?;
    Ok(())
}

pub(super) fn forget_if_deleted(tx: &Transaction<'_>, id: &MessageId) -> Result<()> {
    tx.execute(
        "DELETE FROM voice_notes WHERE message_id=?1 AND EXISTS \
         (SELECT 1 FROM messages WHERE id=?1 AND deleted=1)",
        [id.as_bytes().as_slice()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_wav_round_trips_and_needs_two_slices() {
        let wav = sample_voice_wav(1_200);
        assert!(wav.len() > MAX_VOICE_CHUNK_BYTES);
        let parsed = parse_voice_wav(&wav).unwrap();
        assert_eq!(parsed.duration_ms, 1_200);
        assert_eq!(parsed.waveform.len(), MAX_WAVEFORM_BARS);
        assert!(parse_voice_wav(b"not a wav").is_err());
    }
}
