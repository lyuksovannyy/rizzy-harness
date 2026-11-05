//! Terse answer styles (output-token savings), added to the system prompt.
//!
//! `Full` and `Ultra` are adapted from the "caveman" skill by Julius Brussee
//! (https://github.com/JuliusBrussee/caveman, Apache-2.0): shortened and
//! reworded for this harness; the rule list and the "when to write plain
//! prose" list are kept. See THIRD_PARTY_NOTICES.md. `Lite` is our own text.
//! Approval prompts are not affected: they are written by the harness, not
//! the model.

use harness_core::savings::Style;

/// Plain, readable, no ceremony. The only style chat sessions get.
const LITE: &str = "Answer style: lead with the answer, then the reason. No greetings, \
filler, hedging or closing offers. Keep code, commands, paths, numbers and error \
messages exactly as they are. Use full sentences for warnings, irreversible actions, \
step-by-step instructions and questions to the user.";

const FULL: &str = "Answer style (terse): every output token costs. Rules:
1. Answer first: thing, action, reason, next step.
2. No greeting, hedging, recap, closer, \"Sure\", \"Let me\".
3. Short words; standard acronyms only, no invented abbreviations.
4. Articles optional, fragments fine. Never drop not/never/no/only/except. Numbers and units exact.
5. One idea per sentence, 20 words max, active voice.
6. Payload verbatim: code blocks unchanged; commands, paths, API names, errors exact. Show changed lines plus 1-2 lines of context, not whole files.
7. Between routine tool calls write nothing; one line per phase, one line with the result.
8. Reply in the user's language.
Write plain full sentences instead for: security warnings; irreversible actions (confirm first); step order a fragment could scramble; a confused user; anything stored outside the chat (code, comments, commits, docs); questions or options for the user.";

const ULTRA: &str = "Each fact once. One word when one word is enough. Drop copulas and \
connectives where meaning survives. The plain-sentence exceptions above still apply.";

/// The system-prompt fragment for a style (empty for `Off`).
pub fn fragment(style: Style) -> String {
    match style {
        Style::Off => String::new(),
        Style::Lite => LITE.into(),
        Style::Full => FULL.into(),
        Style::Ultra => format!("{FULL}\n{ULTRA}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_grow_and_keep_the_safety_exceptions() {
        assert!(fragment(Style::Off).is_empty());
        let (l, f, u) = (
            fragment(Style::Lite),
            fragment(Style::Full),
            fragment(Style::Ultra),
        );
        assert!(l.len() < f.len() && f.len() < u.len());
        for s in [&l, &f, &u] {
            assert!(s.contains("warnings"), "{s}");
            assert!(s.contains("exactly") || s.contains("exact"), "{s}");
        }
        // Lite stays readable: no fragment/article dropping.
        assert!(!l.contains("Articles optional"));
    }
}
