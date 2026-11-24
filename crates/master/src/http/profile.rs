//! A person's own settings over HTTP (README "Profile" and
//! "Personalisation", PLAN.md T74): their preferences, their
//! Personalisation, and their profile pictures.
//!
//! Pictures are PNG, JPEG or WebP by their first bytes (never by a name or
//! a declared type; never SVG or GIF), still (no APNG or animated WebP),
//! with no metadata (EXIF, XMP, IPTC, PNG text: an API upload is not
//! re-encoded by the browser), within a byte cap and pixel bounds read from
//! every header (`harness_core::image::still`, no image is decoded here,
//! T17), stored in the blob
//! store as the account's (encrypted, counted against its storage, gone
//! with it). The browser shrinks them first; these checks hold whatever
//! it sends. An avatar is served to its owner and to the people who share
//! a workspace with them (where the members list shows their name); a
//! banner to its owner alone. Anyone else, the API token included, gets
//! 404, so a picture's existence is not confirmed either.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use harness_store::blobs::NewBlob;
use harness_store::profile_images::ProfileImageSet;
use serde_json::{Value, json};

use super::caller::{ByteCaller, User};
use super::{ApiError, ApiResult};
use crate::auth;
use crate::prefs::{Personalisation, Prefs};
use crate::{Master, storage};

/// A picture slot and its limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Avatar,
    Banner,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::Avatar => "avatar",
            Slot::Banner => "banner",
        }
    }

    /// Bytes at most.
    pub fn max_bytes(self) -> usize {
        match self {
            Slot::Avatar => 256 * 1024,
            Slot::Banner => 1024 * 1024,
        }
    }

    /// Width and height at most, in pixels.
    pub fn max_px(self) -> (u32, u32) {
        match self {
            Slot::Avatar => (256, 256),
            Slot::Banner => (1500, 500),
        }
    }
}

/// The formats a picture may be.
const FORMATS: &[&str] = &["png", "jpeg", "webp"];

/// What a picture's bytes are, checked: its format, type and size in
/// pixels, or why it is refused.
pub fn check_image(slot: Slot, b: &[u8]) -> Result<(&'static str, &'static str, u32, u32), String> {
    if b.len() > slot.max_bytes() {
        return Err(format!(
            "The {} is at most {} KB.",
            slot.name(),
            slot.max_bytes() / 1024
        ));
    }
    let r = harness_core::image::raster(b)
        .filter(|r| FORMATS.contains(&r.format))
        .ok_or_else(|| "Only PNG, JPEG and WebP pictures can be used.".to_string())?;
    // The whole file is walked (no pixel decoded): no metadata, one frame,
    // every header's size within the bounds.
    use harness_core::image::NotStill;
    let (w, h) = harness_core::image::still(b, r.format).map_err(|e| match e {
        NotStill::Metadata => "This picture carries camera or location data. Save it without \
                               metadata, or pick it here in the browser, which removes it."
            .to_string(),
        NotStill::Animated => "Only still pictures can be used, not animated ones.".to_string(),
        NotStill::Malformed => format!("This {} file cannot be read.", r.label),
    })?;
    let (mw, mh) = slot.max_px();
    if w > mw || h > mh {
        return Err(format!(
            "The {} is at most {mw} × {mh} pixels; this one is {w} × {h}.",
            slot.name()
        ));
    }
    Ok((r.format, r.mime, w, h))
}

pub(super) fn routes() -> Router<Master> {
    let image = |slot: Slot| {
        put(move |u: User, m: State<Master>, b: Bytes| upload(u, m, slot, b))
            // The cap and a little over, so an over-size body reads as
            // ours (413) rather than the framework's.
            .layer(DefaultBodyLimit::max(slot.max_bytes() + 1))
            .delete(move |u: User, m: State<Master>| remove(u, m, slot))
    };
    Router::new()
        .route("/api/me/settings", get(get_settings).put(put_settings))
        .route("/api/me/settings/personalisation", put(put_personalisation))
        .route("/api/me/profile-images", get(own_images))
        .route("/api/me/avatar", image(Slot::Avatar))
        .route("/api/me/banner", image(Slot::Banner))
        .route(
            "/api/users/{id}/avatar",
            get(
                |b: ByteCaller, m: State<Master>, h: HeaderMap, p: Path<String>| {
                    picture(b, m, h, p, Slot::Avatar)
                },
            ),
        )
        .route(
            "/api/users/{id}/banner",
            get(
                |b: ByteCaller, m: State<Master>, h: HeaderMap, p: Path<String>| {
                    picture(b, m, h, p, Slot::Banner)
                },
            ),
        )
}

async fn get_settings(User(who): User, State(m): State<Master>) -> ApiResult<Json<Prefs>> {
    Ok(Json(Prefs::load(&m, who.user_id.as_deref()).await))
}

/// Everything but Personalisation, which stays as stored (its own endpoint
/// changes it).
async fn put_settings(
    User(who): User,
    State(m): State<Master>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Prefs>> {
    // The e-mail for commits changes only when the request carries the
    // field (`null` or empty clears it): a page loaded before it was set
    // and saving another setting must not wipe it.
    let carries_email = body
        .as_object()
        .is_some_and(|o| o.contains_key("commit_email"));
    let mut p: Prefs = serde_json::from_value(body)
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    let _g = Prefs::write_lock().await;
    let stored = Prefs::load(&m, who.user_id.as_deref()).await;
    if carries_email {
        p.commit_email = p.commit_email.filter(|e| !e.0.trim().is_empty());
    } else {
        p.commit_email = stored.commit_email;
    }
    p.personalisation = stored.personalisation;
    p.validate(&m)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    // Only an account has a commit identity (T88): the API token's
    // settings hold none, so nothing is stored that no chat could use.
    if who.user_id.is_none() && p.commit_email.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "the API token has no e-mail for commits".into(),
        ));
    }
    p.save(&m, who.user_id.as_deref()).await?;
    Ok(Json(p))
}

/// Personalisation alone; the rest of the settings stay as stored. It
/// applies from the person's next message (each turn's `context_sent`
/// records what that turn was sent).
async fn put_personalisation(
    User(who): User,
    State(m): State<Master>,
    Json(b): Json<Personalisation>,
) -> ApiResult<Json<Prefs>> {
    b.validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let _g = Prefs::write_lock().await;
    let mut p = Prefs::load(&m, who.user_id.as_deref()).await;
    p.personalisation = Personalisation {
        instructions: b
            .instructions
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        ..b
    };
    p.save(&m, who.user_id.as_deref()).await?;
    Ok(Json(p))
}

/// The caller's account; the API token has no profile.
fn account(who: auth::Principal) -> ApiResult<String> {
    who.user_id.ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "the API token has no profile".into(),
        )
    })
}

/// A picture's version in URLs: the start of its SHA-256.
fn version(sha256: &str) -> String {
    sha256.chars().take(16).collect()
}

/// The version of `user`'s picture in `slot`, if they have one (for the
/// lists that show it).
pub(crate) async fn image_version(m: &Master, user: &str, slot: Slot) -> Option<String> {
    let u = user.to_string();
    m.inner
        .mirror
        .call(move |s| s.profile_image(&u, slot.name()))
        .await
        .ok()
        .flatten()
        .map(|b| version(&b.sha256))
}

/// `{"avatar": version | null, "banner": version | null}` of the caller.
async fn own_images(User(who): User, State(m): State<Master>) -> ApiResult<Json<Value>> {
    let uid = account(who)?;
    Ok(Json(json!({
        "avatar": image_version(&m, &uid, Slot::Avatar).await,
        "banner": image_version(&m, &uid, Slot::Banner).await,
    })))
}

async fn upload(
    User(who): User,
    State(m): State<Master>,
    slot: Slot,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let uid = account(who)?;
    if body.len() > slot.max_bytes() {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "The {} is at most {} KB.",
                slot.name(),
                slot.max_bytes() / 1024
            ),
        ));
    }
    let (format, mime, w, h) =
        check_image(slot, &body).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let st = storage::get(&m)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "file storage is off".into()))?;
    storage::check_room(&m, Some(&uid), body.len() as u64)
        .await
        .map_err(|e| ApiError(StatusCode::INSUFFICIENT_STORAGE, e))?;
    let refused = |e: std::io::Error| {
        tracing::warn!(error = %e, "could not write a profile picture");
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not store the picture".into(),
        )
    };
    let writer = storage::TempWriter::start(st).map_err(refused)?;
    if !writer.write(body).await {
        writer.abandon().await;
        return Err(refused(std::io::Error::other("the writer stopped")));
    }
    let written = writer.finish().await.map_err(refused)?;
    let blob = NewBlob {
        blob_id: written.blob_id.clone(),
        owner_id: Some(uid.clone()),
        sha256: written.sha256.clone(),
        size: written.size,
        kind: "image".into(),
        format: format.into(),
        mime: mime.into(),
        display: "img".into(),
        key: written.key.clone(),
        width: Some(w),
        height: Some(h),
    };
    let limits = storage::config(&m).limits(Some(&uid));
    let owner = uid.clone();
    let res = storage::commit(&m, written, move |s| {
        let r = s.profile_image_set(&owner, slot.name(), &blob, limits)?;
        let keep = matches!(r, ProfileImageSet::Stored { new_blob: true, .. });
        Ok((r, keep))
    })
    .await
    .map_err(|(c, e)| ApiError(c, e))?;
    match res {
        ProfileImageSet::OverQuota(q) => Err(ApiError(
            StatusCode::INSUFFICIENT_STORAGE,
            storage::full_message(&q),
        )),
        ProfileImageSet::Stored { sha256, gone, .. } => {
            forget(&m, gone).await;
            tracing::info!(user = %uid, slot = slot.name(), "a profile picture was set");
            Ok(Json(json!({ slot.name(): version(&sha256) })))
        }
    }
}

/// Blob rows that went: the WAL truncated, then their files (as Media
/// deletes do, so the keys leave the live files first).
async fn forget(m: &Master, gone: Vec<String>) {
    if gone.is_empty() {
        return;
    }
    if let Err(e) = m.inner.mirror.call(|s| s.checkpoint()).await {
        tracing::warn!(error = %e, "could not checkpoint after removing a picture");
    }
    storage::unlink(m, gone).await;
}

async fn remove(User(who): User, State(m): State<Master>, slot: Slot) -> ApiResult<Json<Value>> {
    let uid = account(who)?;
    let owner = uid.clone();
    let gone = m
        .inner
        .mirror
        .call(move |s| s.profile_image_delete(&owner, slot.name()))
        .await?;
    forget(&m, gone).await;
    Ok(Json(json!({ slot.name(): null })))
}

async fn picture(
    ByteCaller(who): ByteCaller,
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    slot: Slot,
) -> ApiResult<Response> {
    let gone = || ApiError(StatusCode::NOT_FOUND, "no such picture".into());
    let viewer = who.user_id.clone().ok_or_else(gone)?;
    let allowed = viewer == id
        || (slot == Slot::Avatar && {
            let (a, b) = (viewer.clone(), id.clone());
            m.inner
                .mirror
                .call(move |s| s.share_a_workspace(&a, &b))
                .await?
        });
    if !allowed {
        return Err(gone());
    }
    let user = id.clone();
    let row = m
        .inner
        .mirror
        .call(move |s| s.profile_image(&user, slot.name()))
        .await?
        .ok_or_else(gone)?;
    let st = storage::get(&m).ok_or_else(gone)?;
    let path = st.path(&row.blob_id).ok_or_else(gone)?;
    let blob = row.blob_id.clone();
    let key = m
        .inner
        .mirror
        .call(move |s| s.blob_key(&blob))
        .await?
        .ok_or_else(gone)?;
    // `no-store` like every API answer (`response.rs`, T25: a cached copy
    // would outlive a logout or a removed picture); the ETag is the
    // picture's hash, and the `?v=` the lists add (its version) changes
    // with it.
    Ok(crate::blobs::serve(
        &headers,
        crate::blobs::Serve {
            blob: &row,
            path,
            key,
            name: slot.name(),
            inline: true,
        },
    )
    .await)
}
