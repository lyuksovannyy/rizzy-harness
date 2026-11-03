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
}

impl Transcriber {
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
