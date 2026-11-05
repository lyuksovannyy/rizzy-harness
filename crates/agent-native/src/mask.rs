//! Observation masking: older tool outputs are replaced by a one-line note.
//! Research on SWE-bench agents ("The Complexity Trap", JetBrains, 2025)
//! found this halves cost without lowering the solve rate, matching LLM
//! summarisation at no extra calls.
//!
//! Masking happens in batches of `keep`: the set of hidden outputs only
//! changes once every `keep` new outputs, so the provider's prompt cache is
//! invalidated rarely rather than on every step. The choice is a pure
//! function of the history, so a later turn masks exactly the same outputs.

use std::collections::HashMap;

use crate::provider::ChatMsg;

/// Outputs this short are never masked: the note would save nothing.
const MIN_CHARS: usize = 400;

/// Mask all but the newest `keep..2*keep` large tool outputs. Returns how
/// many were masked.
pub fn mask_old_outputs(msgs: &mut [ChatMsg], keep: usize) -> usize {
    let keep = keep.max(1);
    let mut calls: HashMap<String, (String, String)> = HashMap::new();
    for m in msgs.iter() {
        if let ChatMsg::Assistant { tool_calls, .. } = m {
            for c in tool_calls {
                let input = c.input.to_string();
                let short: String = input.chars().take(160).collect();
                calls.insert(c.call_id.clone(), (c.tool.clone(), short));
            }
        }
    }
    let big: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, ChatMsg::Tool { content, .. } if content.len() >= MIN_CHARS))
        .map(|(i, _)| i)
        .collect();
    if big.len() <= keep {
        return 0;
    }
    let hide = (big.len() - keep) / keep * keep;
    for &i in &big[..hide] {
        if let ChatMsg::Tool { call_id, content } = &mut msgs[i] {
            let lines = content.lines().count();
            let (tool, input) = calls
                .get(call_id)
                .cloned()
                .unwrap_or_else(|| ("a tool".into(), String::new()));
            *content = format!(
                "[older output hidden to save tokens: `{tool}` {input} returned {lines} lines. \
                 Run it again if you need it.]"
            );
        }
    }
    hide
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::ToolCallRef;

    fn convo(n: usize) -> Vec<ChatMsg> {
        let mut v = vec![ChatMsg::System("s".into()), ChatMsg::User("u".into())];
        for i in 0..n {
            v.push(ChatMsg::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: format!("c{i}"),
                    tool: "read".into(),
                    input: serde_json::json!({"path": format!("f{i}.rs")}),
                }],
            });
            v.push(ChatMsg::Tool {
                call_id: format!("c{i}"),
                content: format!("line\n{}", "x".repeat(500)),
            });
        }
        v
    }

    fn hidden(v: &[ChatMsg]) -> Vec<bool> {
        v.iter()
            .filter_map(|m| match m {
                ChatMsg::Tool { content, .. } => Some(content.starts_with("[older output hidden")),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn masks_in_batches_oldest_first() {
        let mut v = convo(7);
        assert_eq!(mask_old_outputs(&mut v, 4), 0, "7 < 2*4: nothing yet");
        let mut v = convo(8);
        assert_eq!(mask_old_outputs(&mut v, 4), 4);
        assert_eq!(
            hidden(&v),
            [true, true, true, true, false, false, false, false]
        );
        // One more output does not change what is hidden (cache stays valid).
        let mut w = convo(9);
        assert_eq!(mask_old_outputs(&mut w, 4), 4);
        assert_eq!(&v[..18], &w[..18]);
    }

    #[test]
    fn the_note_names_the_call_and_small_outputs_stay() {
        let mut v = convo(4);
        v.push(ChatMsg::Tool {
            call_id: "tiny".into(),
            content: "ok".into(),
        });
        mask_old_outputs(&mut v, 1);
        let ChatMsg::Tool { content, .. } = &v[3] else {
            panic!()
        };
        assert!(
            content.contains("`read`") && content.contains("f0.rs") && content.contains("2 lines")
        );
        assert!(matches!(v.last(), Some(ChatMsg::Tool { content, .. }) if content == "ok"));
    }
}
