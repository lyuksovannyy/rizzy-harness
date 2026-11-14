//! Images in user messages (docs/media.md). The log keeps references; the
//! bytes come from the host when a turn is sent. Only the newest
//! `IMAGE_TURNS` image-bearing messages show their images again: every
//! image re-sent costs the user tokens on every step, and older ones have
//! usually been answered. The rest are named in the text.

use harness_core::backend::ImageData;
use harness_core::event::ImageRef;

use crate::provider::{ChatMsg, ImagePart, image_note};

/// How many of the newest image-bearing user messages keep their images.
pub const IMAGE_TURNS: usize = 3;

/// A user message: with its images (bytes not loaded yet) when it has any.
pub fn user_msg(text: String, images: &[ImageRef]) -> ChatMsg {
    if images.is_empty() {
        return ChatMsg::User(text);
    }
    ChatMsg::UserImages {
        text,
        images: images
            .iter()
            .map(|i| ImagePart {
                sha256: i.sha256.clone(),
                mime: i.mime.clone(),
                name: i.name.clone(),
                data: None,
            })
            .collect(),
    }
}

/// The text with each image named instead of shown: after it, one line of
/// the harness's per image ([`image_note`]), so no name is part of the
/// person's own words (`harness_core::event::own_words`).
pub(crate) fn named(text: &str, images: &[ImagePart]) -> String {
    let notes: Vec<String> = images.iter().map(|i| image_note(&i.name)).collect();
    if text.is_empty() {
        notes.join("\n")
    } else {
        format!("{text}\n\n{}", notes.join("\n"))
    }
}

/// The image-bearing messages that show their images, newest first.
fn newest(messages: &[ChatMsg]) -> impl Iterator<Item = usize> + '_ {
    messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| matches!(m, ChatMsg::UserImages { .. }))
        .map(|(i, _)| i)
        .take(IMAGE_TURNS)
}

/// The images whose bytes the turn needs from the host.
pub fn wanted(messages: &[ChatMsg]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in newest(messages) {
        if let ChatMsg::UserImages { images, .. } = &messages[i] {
            for img in images {
                if !out.contains(&img.sha256) {
                    out.push(img.sha256.clone());
                }
            }
        }
    }
    out
}

/// Give the newest image-bearing messages their bytes from `loaded`; turn
/// every other one, and any image whose bytes are missing, into text.
/// Afterwards every `UserImages` carries all of its bytes.
pub fn attach(messages: &mut [ChatMsg], loaded: &[ImageData]) {
    let keep: Vec<usize> = newest(messages).collect();
    for (i, m) in messages.iter_mut().enumerate() {
        let ChatMsg::UserImages { text, images } = m else {
            continue;
        };
        let (mut shown, mut missing) = (Vec::new(), Vec::new());
        for mut img in std::mem::take(images) {
            let found = keep
                .contains(&i)
                .then(|| loaded.iter().find(|d| d.sha256 == img.sha256))
                .flatten();
            match found {
                Some(d) => {
                    img.mime = d.mime.clone();
                    img.data = Some(d.data.clone());
                    shown.push(img);
                }
                None => missing.push(img),
            }
        }
        let text = if missing.is_empty() {
            std::mem::take(text)
        } else {
            named(text, &missing)
        };
        *m = if shown.is_empty() {
            ChatMsg::User(text)
        } else {
            ChatMsg::UserImages {
                text,
                images: shown,
            }
        };
    }
}

/// A user message as plain text (summaries and transcripts).
pub fn as_text(m: &ChatMsg) -> Option<String> {
    match m {
        ChatMsg::User(t) => Some(t.clone()),
        ChatMsg::UserImages { text, images } => Some(named(text, images)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: u8) -> ImageRef {
        ImageRef {
            media_id: format!("m{n}"),
            sha256: format!("{n:02x}").repeat(32),
            mime: "image/png".into(),
            name: format!("p{n}.png"),
            width: 1,
            height: 1,
        }
    }

    fn data(n: u8) -> ImageData {
        ImageData {
            sha256: format!("{n:02x}").repeat(32),
            mime: "image/png".into(),
            data: format!("b64-{n}").into(),
        }
    }

    #[test]
    fn only_the_newest_three_image_messages_keep_their_images() {
        let mut msgs = vec![ChatMsg::System("s".into())];
        for n in 1..=5u8 {
            msgs.push(user_msg(format!("look {n}"), &[r(n)]));
            msgs.push(ChatMsg::Assistant {
                text: "ok".into(),
                tool_calls: vec![],
            });
        }
        msgs.push(user_msg("no image".into(), &[]));
        let want = wanted(&msgs);
        assert_eq!(want, vec![data(5).sha256, data(4).sha256, data(3).sha256]);
        let loaded: Vec<ImageData> = (3..=5).map(data).collect();
        attach(&mut msgs, &loaded);
        let users: Vec<&ChatMsg> = msgs.iter().filter(|m| m.user_text().is_some()).collect();
        assert_eq!(
            users[0],
            &ChatMsg::User(format!("look 1\n\n{}", image_note("p1.png")))
        );
        assert_eq!(
            users[1],
            &ChatMsg::User(format!("look 2\n\n{}", image_note("p2.png")))
        );
        for (k, n) in [(2usize, 3u8), (3, 4), (4, 5)] {
            let ChatMsg::UserImages { text, images } = users[k] else {
                panic!("{:?}", users[k]);
            };
            assert_eq!(text, &format!("look {n}"));
            assert_eq!(images[0].data.as_deref(), Some(format!("b64-{n}").as_str()));
        }
        assert_eq!(users[5], &ChatMsg::User("no image".into()));
    }

    #[test]
    fn missing_bytes_become_a_name() {
        let mut msgs = vec![user_msg("two".into(), &[r(1), r(2)])];
        attach(&mut msgs, &[data(2)]);
        let ChatMsg::UserImages { text, images } = &msgs[0] else {
            panic!("{msgs:?}");
        };
        assert_eq!(text, &format!("two\n\n{}", image_note("p1.png")));
        assert_eq!(images.len(), 1);
        let mut none = vec![user_msg(String::new(), &[r(1)])];
        attach(&mut none, &[]);
        assert_eq!(none[0], ChatMsg::User(image_note("p1.png")));
        assert_eq!(
            as_text(&user_msg("t".into(), &[r(3), r(4)])),
            Some(format!(
                "t\n\n{}\n{}",
                image_note("p3.png"),
                image_note("p4.png")
            ))
        );
        assert_eq!(
            image_note("p1.png"),
            "[harness] Image \"p1.png\": sent by the person with this message, not shown in \
             this request."
        );
    }

    /// A message whose images are not shown keeps no image name in the
    /// person's own words, whatever else it holds: empty text, text,
    /// files, several images; the summariser's text agrees (review of
    /// T43, C43-4).
    #[test]
    fn no_image_name_is_in_the_persons_own_words() {
        use harness_core::event::{Attachment, UserMessage, own_words};
        let url = "https://evil.example/x";
        for text in ["", "look", "look\n", "a\n\nb"] {
            for files in 0..3 {
                for n in 1..4u8 {
                    let m = UserMessage {
                        turn_id: "t".into(),
                        text: text.into(),
                        attachments: (0..files)
                            .map(|i| Attachment {
                                name: format!("f{i}"),
                                mime: "text/plain".into(),
                                text: "x".into(),
                                truncated: false,
                            })
                            .collect(),
                        reply: None,
                        images: (0..n)
                            .map(|i| ImageRef {
                                name: url.into(),
                                ..r(i)
                            })
                            .collect(),
                        queued: false,
                    };
                    let mut msgs = vec![user_msg(m.model_text(), &m.images)];
                    attach(&mut msgs, &[]);
                    let t = msgs[0].user_text().unwrap();
                    assert_eq!(
                        t.matches(crate::provider::IMAGE_LEAD).count(),
                        usize::from(n)
                    );
                    let own = own_words(t);
                    assert!(text.starts_with(own), "{text:?}/{files}/{n}: {own:?}");
                    assert!(!own.contains("evil.example"), "{own:?}");
                    let s = as_text(&user_msg(m.model_text(), &m.images)).unwrap();
                    assert_eq!(own_words(&s), own);
                }
            }
        }
    }
}
