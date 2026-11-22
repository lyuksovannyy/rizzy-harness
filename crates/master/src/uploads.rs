//! Files the model cannot read as text, attached to a message (PLAN.md
//! T94, docs/plan/X.53.md). The master only stores and forwards: the bytes
//! are the owner's blob until the message is sent (`http/attachments.rs`
//! makes it), and then they are pushed to the chat's node, which saves
//! them into the chat's folder under its own rules. The master runs no
//! file tool and writes nothing into a folder.
//!
//! The push is `session.upload` chunks of at most [`MAX_CHUNK`] bytes, one
//! in flight (each is acked), in the chat's lane ahead of the prompt that
//! names them; a file is never held whole, and the link's frame is never
//! approached. What the node says it saves (`upload-max:`,
//! `uploads-per-message:` in its hello) is what is checked here first; the
//! node checks again and decides.

use base64::Engine as _;
use harness_core::SessionId;
use harness_protocol::{MAX_CHUNK, MasterToNode, UploadRef};
use harness_store::SessionRow;
use harness_store::chat_uploads::TakenUpload;

use crate::{CommandError, Master, storage};

/// Unsent files one account may hold. Each may be tens of MiB on disk, so
/// as many as text attachments (`attachments::MAX_UNSENT`), two messages' worth.
pub const MAX_UNSENT: usize = 2 * crate::attachments::MAX_PER_MESSAGE;

/// The number the master allows a file before it knows the chat's node:
/// the home composer uploads before the chat exists.
pub const DEFAULT_MAX: u64 = harness_protocol::DEFAULT_UPLOAD_MAX_MB * 1024 * 1024;

/// What `node_id` saves of attached files, from its latest hello.
pub fn limits(m: &Master, node_id: &str) -> Option<harness_protocol::UploadLimits> {
    m.node_capabilities(node_id)
        .and_then(|c| harness_protocol::upload_limits_of(&c))
}

/// Why this chat takes no file the model cannot read as text, or `None`
/// when it may: an agentic chat of its own (never a plain chat, never a
/// sub-agent's chat).
pub fn refusal(row: &SessionRow) -> Option<String> {
    if row.profile.as_deref() == Some("chat") {
        return Some(crate::attachments::NO_TOOLS.into());
    }
    if row.parent_id.is_some() {
        return Some("a sub-agent's chat takes no attached files".into());
    }
    None
}

/// The checks a send makes before anything is pushed: the chat may take
/// files, its node can save them, and their number and sizes are within
/// what the node announced. Sentences for the person.
pub fn check(m: &Master, row: &SessionRow, ups: &[TakenUpload]) -> Result<(), CommandError> {
    if let Some(why) = refusal(row) {
        return Err(CommandError::Rejected(why));
    }
    let Some(l) = limits(m, &row.node_id) else {
        return Err(CommandError::Rejected(
            "this chat's node cannot save attached files: update the node (it does not announce \
             the `uploads` capability)"
                .into(),
        ));
    };
    if ups.len() > l.per_message {
        return Err(CommandError::Rejected(format!(
            "this chat's node takes at most {} attached files per message",
            l.per_message
        )));
    }
    let max = l.max_mb * 1024 * 1024;
    if let Some(u) = ups.iter().find(|u| u.size > max) {
        return Err(CommandError::Rejected(format!(
            "{}: this chat's node saves files up to {} MB",
            u.name, l.max_mb
        )));
    }
    Ok(())
}

/// One chunk of a stored file: `len` plaintext bytes at `offset`, read
/// and decrypted off the async threads.
async fn read_chunk(
    m: &Master,
    u: &TakenUpload,
    key: &[u8],
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, CommandError> {
    let gone = || CommandError::Rejected(format!("{}: the file is gone; attach it again", u.name));
    let path = storage::get(m)
        .and_then(|st| st.path(&u.blob_id))
        .ok_or_else(gone)?;
    let (id, key, size) = (u.blob_id.clone(), key.to_vec(), u.size);
    tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut r = crate::blobs::Reader::open(&path, &id, &key, size)?;
        let mut out = Vec::with_capacity(len as usize);
        r.range(offset, offset + len - 1, |b| {
            out.extend_from_slice(b);
            Ok(())
        })?;
        Ok(out)
    })
    .await
    .map_err(|e| CommandError::Rejected(format!("could not read the file: {e}")))?
    .map_err(|e| {
        tracing::warn!(error = %e, "a stored upload could not be read");
        gone()
    })
}

/// Push `ups` to `node_id` for chat `sid`, a chunk at a time, each acked.
/// Returns the references the prompt names them by. An answer that does
/// not come is a refusal here: the prompt has not gone, so nothing of the
/// message can have been taken and the uploads may be given back.
pub async fn push(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    ups: &[TakenUpload],
) -> Result<Vec<UploadRef>, CommandError> {
    let mut refs = Vec::with_capacity(ups.len());
    for u in ups {
        let id = u.blob_id.clone();
        let key = m
            .inner
            .mirror
            .call(move |s| s.blob_key(&id))
            .await?
            .ok_or_else(|| {
                CommandError::Rejected(format!("{}: the file is gone; attach it again", u.name))
            })?;
        let mut offset = 0u64;
        while offset < u.size {
            let len = (u.size - offset).min(u64::from(MAX_CHUNK));
            let data = read_chunk(m, u, &key, offset, len).await?;
            let data = base64::engine::general_purpose::STANDARD.encode(&data);
            let (upload_id, sha256, size, at) =
                (u.upload_id.clone(), u.sha256.clone(), u.size, offset);
            m.send_command(node_id, move |req_id| MasterToNode::UploadChunk {
                req_id,
                session_id: sid,
                upload_id,
                offset: at,
                size,
                sha256,
                data,
            })
            .await
            .map_err(|e| match e {
                CommandError::Timeout => CommandError::Rejected(
                    "the node did not answer while the file was being sent; send it again".into(),
                ),
                e => e,
            })?;
            offset += len;
        }
        refs.push(UploadRef {
            upload_id: u.upload_id.clone(),
            name: crate::attachments::clean_name(&u.name),
            mime: u.mime.clone(),
            size: u.size,
            sha256: u.sha256.clone(),
        });
    }
    Ok(refs)
}

/// Extra wait for a prompt's ack per MiB of files it names: the node
/// copies, hashes and syncs them before it answers (PLAN.md T94). Generous
/// for a slow disk (2 MiB/s); only a ceiling, an answer ends the wait.
pub const ACK_PER_MIB: std::time::Duration = std::time::Duration::from_millis(500);

/// The wait to add to a prompt's ack for files of `sizes` bytes.
pub fn ack_extra(sizes: &[u64]) -> std::time::Duration {
    let total: u64 = sizes.iter().fold(0u64, |a, s| a.saturating_add(*s));
    ACK_PER_MIB.saturating_mul(u32::try_from(total.div_ceil(1024 * 1024)).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_ack_wait_grows_with_the_bytes_a_prompt_carries() {
        assert_eq!(ack_extra(&[]), Duration::ZERO);
        assert_eq!(ack_extra(&[1]), Duration::from_millis(500));
        assert_eq!(ack_extra(&[1024 * 1024, 1]), Duration::from_secs(1));
        // Ten files of 10 MiB: 50 s on top of the base.
        assert_eq!(ack_extra(&[10 << 20; 10]), Duration::from_secs(50));
        // Never overflows.
        assert!(ack_extra(&[u64::MAX, u64::MAX]) > Duration::from_secs(1));
    }
}
