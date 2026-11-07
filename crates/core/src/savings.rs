//! Token-saving methods. Every method can be switched off. The master holds
//! the admin's choice ([`TokenSavings`], `[savings]` in config.toml) and
//! resolves it per turn into [`Savings`], which travels with the prompt.
//! Nothing here widens what a node allows: graphify also needs the node's
//! own `node.toml` to permit it.

use serde::{Deserialize, Serialize};

/// Answer style (terse output). Chat sessions are capped at `Lite`: answers
/// there are read by people, and the heavier styles trade clarity for tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Off,
    Lite,
    Full,
    Ultra,
}

/// What the admin chose (config.toml `[savings]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TokenSavings {
    /// Mark the stable start of each request cacheable (Anthropic; OpenAI
    /// caches by itself).
    pub prompt_cache: bool,
    /// Replace tool outputs older than the last `keep_tool_outputs` with a
    /// short placeholder (observation masking).
    pub mask_old_outputs: bool,
    pub keep_tool_outputs: u32,
    /// Strip colours/progress bars, collapse repeats, failures-only test
    /// output; the full output stays on the node (`tool_output` tool).
    pub filter_shell_output: bool,
    /// File reads in windows with an "N lines more" note; grep result caps.
    pub bounded_reads: bool,
    /// `web_fetch` keeps the main article as Markdown instead of all page text.
    pub web_readability: bool,
    /// Off or Lite only (Full/Ultra are clamped to Lite).
    pub chat_style: Style,
    pub agentic_style: Style,
    /// A ranked map of the repository's symbols in the first request.
    pub repo_map: bool,
    pub repo_map_tokens: u32,
    /// `outline` tool: a file's signatures without bodies.
    pub outline_tool: bool,
    /// `graph_query` tool backed by graphify (only on nodes that allow it
    /// and have it installed).
    pub graphify: bool,
}

impl Default for TokenSavings {
    fn default() -> Self {
        Self {
            prompt_cache: true,
            mask_old_outputs: true,
            keep_tool_outputs: 8,
            filter_shell_output: true,
            bounded_reads: true,
            web_readability: true,
            chat_style: Style::Lite,
            agentic_style: Style::Lite,
            repo_map: false,
            repo_map_tokens: 1024,
            outline_tool: true,
            graphify: false,
        }
    }
}

impl TokenSavings {
    /// The settings for one turn of a chat or agentic session.
    pub fn for_turn(&self, chat: bool) -> Savings {
        Savings {
            prompt_cache: self.prompt_cache,
            keep_tool_outputs: self
                .mask_old_outputs
                .then_some(self.keep_tool_outputs.max(1)),
            filter_shell_output: self.filter_shell_output && !chat,
            bounded_reads: self.bounded_reads,
            web_readability: self.web_readability,
            style: if chat {
                self.chat_style.min(Style::Lite)
            } else {
                self.agentic_style
            },
            repo_map_tokens: (self.repo_map && !chat)
                .then_some(self.repo_map_tokens.clamp(256, 8192)),
            outline_tool: self.outline_tool && !chat,
            graphify: self.graphify && !chat,
        }
    }
}

/// The methods in effect for one turn. The default (a prompt without
/// settings) is everything off.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Savings {
    pub prompt_cache: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_tool_outputs: Option<u32>,
    pub filter_shell_output: bool,
    pub bounded_reads: bool,
    pub web_readability: bool,
    pub style: Style,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_map_tokens: Option<u32>,
    pub outline_tool: bool,
    pub graphify: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_is_capped_at_lite_and_gets_no_agentic_methods() {
        let t = TokenSavings {
            chat_style: Style::Ultra,
            agentic_style: Style::Ultra,
            repo_map: true,
            graphify: true,
            ..Default::default()
        };
        let c = t.for_turn(true);
        assert_eq!(c.style, Style::Lite);
        assert!(!c.filter_shell_output && !c.outline_tool && !c.graphify);
        assert_eq!(c.repo_map_tokens, None);
        let a = t.for_turn(false);
        assert_eq!(a.style, Style::Ultra);
        assert!(a.graphify && a.outline_tool);
        assert_eq!(a.repo_map_tokens, Some(1024));
    }

    #[test]
    fn every_method_can_be_off() {
        let off = TokenSavings {
            prompt_cache: false,
            mask_old_outputs: false,
            filter_shell_output: false,
            bounded_reads: false,
            web_readability: false,
            chat_style: Style::Off,
            agentic_style: Style::Off,
            repo_map: false,
            outline_tool: false,
            graphify: false,
            ..Default::default()
        };
        assert_eq!(off.for_turn(false), Savings::default());
        assert_eq!(off.for_turn(true), Savings::default());
    }

    #[test]
    fn config_parses_partially() {
        let t: TokenSavings = toml_like(r#"{"agentic_style":"full","graphify":true}"#);
        assert_eq!(t.agentic_style, Style::Full);
        assert!(t.graphify && t.prompt_cache);
    }

    fn toml_like(s: &str) -> TokenSavings {
        serde_json::from_str(s).unwrap()
    }
}
