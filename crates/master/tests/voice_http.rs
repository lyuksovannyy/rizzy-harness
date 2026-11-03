//! Dictation is forwarded to the transcription service; nothing is stored.

use axum::extract::Multipart;
use axum::routing::post;
use harness_core::Secret;
use harness_master::auth::Auth;
use harness_master::voice::Transcriber;
use harness_master::{Master, router};
use harness_store::Store;
use serde_json::{Value, json};

#[tokio::test]
async fn transcribe_forwards_audio_and_returns_text() {
    let fake = axum::Router::new().route(
        "/v1/audio/transcriptions",
        post(
            |headers: axum::http::HeaderMap, mut mp: Multipart| async move {
                assert_eq!(headers["authorization"], "Bearer vk");
                let (mut model, mut bytes, mut name) = (String::new(), 0usize, String::new());
                while let Some(f) = mp.next_field().await.unwrap() {
                    match f.name().unwrap() {
                        "model" => model = f.text().await.unwrap(),
                        "file" => {
                            name = f.file_name().unwrap().to_string();
                            bytes = f.bytes().await.unwrap().len();
                        }
                        _ => {}
                    }
                }
                axum::Json(json!({"text": format!(" heard {bytes} bytes as {name} via {model} ")}))
            },
        ),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fa = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, fake).await.unwrap() });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    let c = reqwest::Client::new();
    let base = format!("http://{addr}");
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });

    // Off until configured.
    let v: Value = c
        .get(format!("{base}/api/features"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["voice"], false);
    let r = c
        .post(format!("{base}/api/transcribe"))
        .bearer_auth("tok")
        .body(vec![1u8; 10])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 503);

    master.set_transcriber(Transcriber::new(
        &format!("http://{fa}/v1"),
        "small",
        Some(Secret::new("vk".into())),
    ));
    let v: Value = c
        .get(format!("{base}/api/features"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["voice"], true);
    let r = c
        .post(format!("{base}/api/transcribe"))
        .bearer_auth("tok")
        .header("content-type", "audio/ogg;codecs=opus")
        .body(vec![7u8; 1234])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["text"], "heard 1234 bytes as audio.ogg via small");
    // Unauthenticated callers cannot use it.
    let r = c
        .post(format!("{base}/api/transcribe"))
        .body(vec![1u8; 10])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}
