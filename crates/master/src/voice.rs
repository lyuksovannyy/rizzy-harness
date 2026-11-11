//! Dictation: forward recorded audio to the configured transcription
//! endpoint. Audio is not stored.

use std::time::Duration;

use harness_core::Secret;

/// Largest recording accepted (about 25 minutes of Opus).
pub const MAX_AUDIO: usize = 25 * 1024 * 1024;

pub struct Transcriber {
    client: reqwest::Client,
    url: String,
    model: String,
    key: Option<Secret<String>>,
    limits: Limits,
}

/// Per-account dictation limits (`[voice] daily_requests`, `daily_mb`; 0 =
/// unlimited). The upstream service is paid per minute of audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub daily_requests: u32,
    pub daily_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            daily_requests: DEFAULT_DAILY_REQUESTS,
            daily_bytes: DEFAULT_DAILY_MB * 1024 * 1024,
        }
    }
}

pub const DEFAULT_DAILY_REQUESTS: u32 = 200;
/// About seven hours of the UI's Opus recordings.
pub const DEFAULT_DAILY_MB: u64 = 100;

/// Why a dictation is refused before it is forwarded (both 429).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("one dictation at a time; wait for the last one to finish")]
    Busy,
    #[error("daily dictation limit reached; it resets at midnight UTC")]
    DailyLimit,
}

/// Who is transcribing now and what each account used today (UTC). In
/// memory only: a restart (an admin's act, never a user's) resets the day's
/// counts, which at worst gives each account one more day's allowance.
#[derive(Debug, Default)]
pub struct Usage {
    state: std::sync::Mutex<UsageState>,
}

#[derive(Debug, Default)]
struct UsageState {
    day: i64,
    used: std::collections::HashMap<String, (u32, u64)>,
    busy: std::collections::HashSet<String>,
}

/// Held while one account's dictation runs; frees the account on drop
/// (success, error, or the request dropped).
pub struct Slot {
    usage: std::sync::Arc<Usage>,
    user: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut s = self.usage.state.lock().unwrap_or_else(|e| e.into_inner());
        s.busy.remove(&self.user);
    }
}

impl Usage {
    /// Start a dictation of `bytes` for `user` (the API token is `""`): one
    /// at a time per account, within the daily limits. The request is
    /// counted now, before it is forwarded, so a failed upstream call counts
    /// too (it may still be billed).
    pub fn begin(
        self: &std::sync::Arc<Self>,
        user: &str,
        bytes: u64,
        limits: Limits,
        now_ms: i64,
    ) -> Result<Slot, Refusal> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let day = now_ms.div_euclid(86_400_000);
        if s.day != day {
            s.day = day;
            s.used.clear();
        }
        if s.busy.contains(user) {
            return Err(Refusal::Busy);
        }
        let (n, b) = s.used.get(user).copied().unwrap_or((0, 0));
        let over_n = limits.daily_requests != 0 && n >= limits.daily_requests;
        let over_b = limits.daily_bytes != 0 && b.saturating_add(bytes) > limits.daily_bytes;
        if over_n || over_b {
            return Err(Refusal::DailyLimit);
        }
        s.used
            .insert(user.to_string(), (n + 1, b.saturating_add(bytes)));
        s.busy.insert(user.to_string());
        Ok(Slot {
            usage: self.clone(),
            user: user.to_string(),
        })
    }
}

impl Transcriber {
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn new(url: &str, model: &str, key: Option<Secret<String>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("static client config"),
            url: url.trim_end_matches('/').to_string(),
            model: if model.is_empty() {
                "whisper-1".into()
            } else {
                model.into()
            },
            key,
            limits: Limits::default(),
        }
    }

    pub async fn transcribe(&self, audio: Vec<u8>, mime: &str) -> Result<String, String> {
        let ext = if mime.contains("ogg") {
            "ogg"
        } else if mime.contains("mp4") || mime.contains("m4a") {
            "m4a"
        } else if mime.contains("wav") {
            "wav"
        } else {
            "webm"
        };
        let part = reqwest::multipart::Part::bytes(audio)
            .file_name(format!("audio.{ext}"))
            .mime_str(mime)
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .part("file", part);
        let mut rb = self
            .client
            .post(format!("{}/audio/transcriptions", self.url))
            .multipart(form);
        if let Some(k) = &self.key {
            rb = rb.bearer_auth(k.expose());
        }
        let r = rb
            .send()
            .await
            .map_err(|e| format!("transcription failed: {e}"))?;
        let status = r.status();
        if !status.is_success() {
            return Err(format!("transcription service answered {status}"));
        }
        let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
        v["text"]
            .as_str()
            .map(|t| t.trim().to_string())
            .ok_or_else(|| "transcription service returned no text".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const DAY: i64 = 86_400_000;

    #[test]
    fn one_at_a_time_per_account() {
        let u = Arc::new(Usage::default());
        let l = Limits::default();
        let a = u.begin("alex", 10, l, DAY).unwrap();
        assert_eq!(u.begin("alex", 10, l, DAY).err(), Some(Refusal::Busy));
        // Another account is not affected.
        let _s = u.begin("sam", 10, l, DAY).unwrap();
        drop(a);
        assert!(u.begin("alex", 10, l, DAY).is_ok());
    }

    #[test]
    fn daily_caps_per_account_reset_at_midnight_utc() {
        let u = Arc::new(Usage::default());
        let l = Limits {
            daily_requests: 2,
            daily_bytes: 100,
        };
        drop(u.begin("alex", 10, l, DAY).unwrap());
        drop(u.begin("alex", 10, l, DAY + 1).unwrap());
        assert_eq!(
            u.begin("alex", 10, l, DAY + 2).err(),
            Some(Refusal::DailyLimit)
        );
        drop(u.begin("sam", 100, l, DAY).unwrap());
        assert_eq!(u.begin("sam", 1, l, DAY).err(), Some(Refusal::DailyLimit));
        // The next UTC day starts afresh.
        drop(u.begin("alex", 10, l, 2 * DAY).unwrap());
        // 0 = unlimited.
        let free = Limits {
            daily_requests: 0,
            daily_bytes: 0,
        };
        for _ in 0..5 {
            drop(u.begin("pat", u64::MAX / 4, free, 2 * DAY).unwrap());
        }
    }
}
