//! Terse answer styles (output-token savings), added to the system prompt.
//!
//! `Full` and `Ultra` are adapted from the "caveman" skill by Julius Brussee
//! (https://github.com/JuliusBrussee/caveman, Apache-2.0): shortened and
//! reworded for this harness; the rule list and the "when to write plain
//! prose" list are kept. See THIRD_PARTY_NOTICES.md. `Lite` is our own text.
//! Approval prompts are not affected: they are written by the harness, not
//! the model.

use harness_core::agent::UiLevel;
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
7. Between routine tool calls write nothing{RULE7_UI}; one line per phase, one line with the result.
8. Reply in the user's language.
Write plain full sentences instead for: security warnings; irreversible actions (confirm first); step order a fragment could scramble; a confused user; anything stored outside the chat (code, comments, commits, docs); questions or options for the user.";

const ULTRA: &str = "Each fact once. One word when one word is enough. Drop copulas and \
connectives where meaning survives. The plain-sentence exceptions above still apply.";

/// Rule 7 of `FULL` where progress blocks render.
const RULE7_UI: &str = ", or keep one ui:progress block updated";

/// The system-prompt fragment for a style (empty for `Off`). `ui` is the
/// turn's block level: rule 7 mentions progress blocks only where they show.
pub fn fragment(style: Style, ui: UiLevel) -> String {
    let full = || FULL.replace("{RULE7_UI}", if ui == UiLevel::Off { "" } else { RULE7_UI });
    match style {
        Style::Off => String::new(),
        Style::Lite => LITE.into(),
        Style::Full => full(),
        Style::Ultra => format!("{}\n{ULTRA}", full()),
    }
}

// What the UI renders and how to write blocks (rich_plan §8), after the
// style fragment. One fixed text per level, so the system prompt stays
// cacheable. The level is enforced when the message is recorded
// (`harness_core::ui::extract`); this only tells the model what will show.
const UI_FORMAT: &str = "Formatting: replies render as Markdown (GFM tables, task lists, footnotes); \
fence code with a language tag. Math: \\( \\) inline and $$ on their own lines for display; never single $. \
Callouts: a blockquote starting [!NOTE], [!TIP], [!IMPORTANT], [!WARNING] or [!CAUTION]. \
Web results carry labels like [S2]: cite them right after the claim; never write a source URL you did \
not get from a tool. Images are not shown.";
const UI_BLOCKS: &str = "Blocks: a fence at the start of a line with info string ui:<type> and one JSON \
object as body; plain text only.";
const UI_INPUT: &str = "ui:choices {id, question, options:[{id, label, description?}], multi?, other?}: \
one tap-to-answer question; end your reply after it. ui:form {id, title, fields:[{id, label, type: \
text|number|choice|toggle|date, required?, options?, min?, max?}]}: at most one choices or form per reply; \
never ask for passwords, keys, tokens or card numbers.";
const UI_DISPLAY: &str = "ui:progress {id, title, steps:[{label, status: pending|active|done|failed|skipped}]}: \
repeat it with the same id after each phase. ui:chart {kind: bar|line, x:{values}, series:[{name, values}]}: \
only numbers from tools or the user.";
const UI_ANSWERS: &str = "Answers arrive as a message starting \"Form answer ·\" or \"Choice answer ·\" that \
names your ids. Prefer prose.";

/// The block instructions for a level (empty for `Off`).
pub fn ui_fragment(level: UiLevel) -> String {
    match level {
        UiLevel::Off => String::new(),
        UiLevel::Display => format!("{UI_FORMAT} {UI_BLOCKS} {UI_DISPLAY} Prefer prose."),
        UiLevel::Rich => format!("{UI_FORMAT} {UI_BLOCKS} {UI_INPUT} {UI_DISPLAY} {UI_ANSWERS}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_grow_and_keep_the_safety_exceptions() {
        assert!(fragment(Style::Off, UiLevel::Rich).is_empty());
        let (l, f, u) = (
            fragment(Style::Lite, UiLevel::Rich),
            fragment(Style::Full, UiLevel::Rich),
            fragment(Style::Ultra, UiLevel::Rich),
        );
        assert!(l.len() < f.len() && f.len() < u.len());
        for s in [&l, &f, &u] {
            assert!(s.contains("warnings"), "{s}");
            assert!(s.contains("exactly") || s.contains("exact"), "{s}");
        }
        // Lite stays readable: no fragment/article dropping.
        assert!(!l.contains("Articles optional"));
        // Rule 7 names progress blocks only where they render.
        assert!(f.contains("nothing, or keep one ui:progress block updated; one line per phase"));
        let plain = fragment(Style::Full, UiLevel::Off);
        assert!(plain.contains("write nothing; one line per phase") && !plain.contains("ui:"));
    }

    #[test]
    fn ui_fragment_per_level() {
        assert!(ui_fragment(UiLevel::Off).is_empty());
        let rich = ui_fragment(UiLevel::Rich);
        let display = ui_fragment(UiLevel::Display);
        for s in [&rich, &display] {
            assert!(
                s.contains("ui:progress") && s.contains("ui:chart") && s.contains("[S2]"),
                "{s}"
            );
            assert!(s.contains(r"\( \)") && s.contains("never single $"), "{s}");
        }
        assert!(
            rich.contains("ui:choices")
                && rich.contains("ui:form")
                && rich.contains("Form answer ·")
        );
        assert!(rich.contains("never ask for passwords"));
        for gone in ["ui:choices", "ui:form", "answer ·"] {
            assert!(!display.contains(gone), "{gone}");
        }
        // About 230 tokens at 4 characters per token.
        assert!(rich.len() < 1300, "{}", rich.len());
    }
}
