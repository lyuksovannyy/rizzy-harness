//! The providers onboarding and settings offer. Fixed presets pin their base
//! URL: the server ignores any URL a client sends for them.

use serde::Serialize;

use crate::config::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    TrustedSource,
    TrainingData,
    DataCollection,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub text: &'static str,
    /// Save unlocks this many seconds after a successful test.
    pub wait_secs: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Preset {
    /// Also the default provider id (model prefix) for non-custom presets.
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ProviderKind,
    /// `None`: the user enters it.
    pub base_url: Option<&'static str>,
    pub url_editable: bool,
    pub needs_key: bool,
    pub warning: Option<Warning>,
    /// The user names it and picks the prefix.
    pub custom: bool,
}

const TRUSTED: Warning = Warning {
    kind: WarningKind::TrustedSource,
    text: "Only connect endpoints you trust: they receive every prompt and file the assistant sends.",
    wait_secs: 0,
};

const fn fixed(id: &'static str, label: &'static str, url: &'static str) -> Preset {
    Preset {
        id,
        label,
        kind: ProviderKind::Openai,
        base_url: Some(url),
        url_editable: false,
        needs_key: true,
        warning: None,
        custom: false,
    }
}

pub const PRESETS: &[Preset] = &[
    fixed("openai", "OpenAI (ChatGPT)", "https://api.openai.com/v1"),
    Preset {
        kind: ProviderKind::Anthropic,
        ..fixed(
            "anthropic",
            "Anthropic (Claude)",
            "https://api.anthropic.com/v1",
        )
    },
    fixed(
        "gemini",
        "Google Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
    ),
    fixed("mistral", "Mistral", "https://api.mistral.ai/v1"),
    Preset {
        warning: Some(Warning {
            kind: WarningKind::DataCollection,
            text: "DeepSeek may collect and store prompts and outputs.",
            wait_secs: 10,
        }),
        ..fixed("deepseek", "DeepSeek", "https://api.deepseek.com/v1")
    },
    Preset {
        warning: Some(Warning {
            kind: WarningKind::TrainingData,
            text: "NVIDIA's free NIM endpoints may use your prompts and outputs to train models.",
            wait_secs: 10,
        }),
        ..fixed(
            "nvidia_nim",
            "NVIDIA NIM",
            "https://integrate.api.nvidia.com/v1",
        )
    },
    fixed("moonshot", "Kimi (Moonshot)", "https://api.moonshot.ai/v1"),
    fixed("zai", "Z.ai (GLM)", "https://api.z.ai/api/paas/v4"),
    fixed("xai", "xAI (Grok)", "https://api.x.ai/v1"),
    fixed("groq", "Groq", "https://api.groq.com/openai/v1"),
    fixed("openrouter", "OpenRouter", "https://openrouter.ai/api/v1"),
    fixed("together", "Together AI", "https://api.together.xyz/v1"),
    fixed(
        "fireworks",
        "Fireworks",
        "https://api.fireworks.ai/inference/v1",
    ),
    Preset {
        url_editable: true,
        needs_key: false,
        ..fixed("ollama", "Ollama", "http://localhost:11434/v1")
    },
    // Local OpenAI-compatible servers (llama.cpp, vLLM, LM Studio) often
    // take no key, so none is required here.
    Preset {
        base_url: None,
        url_editable: true,
        needs_key: false,
        warning: Some(TRUSTED),
        custom: true,
        ..fixed("custom_openai", "OpenAI-compatible (custom)", "")
    },
    Preset {
        kind: ProviderKind::Anthropic,
        base_url: None,
        url_editable: true,
        warning: Some(TRUSTED),
        custom: true,
        ..fixed("custom_anthropic", "Anthropic-compatible (custom)", "")
    },
];

/// The built-in scripted provider (not listed: onboarding's "Skip").
pub const DEMO: &str = "demo";

/// Ids a provider cannot be saved under: `/api/admin/providers/<id>` is an
/// action for these.
pub const RESERVED_IDS: &[&str] = &[DEMO, "test"];

pub fn get(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// URLs compare without a trailing slash.
pub fn same_url(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

/// The non-custom preset of `kind` whose fixed URL is `url`.
pub fn by_url(kind: ProviderKind, url: &str) -> Option<&'static Preset> {
    PRESETS
        .iter()
        .find(|p| !p.custom && p.kind == kind && p.base_url.is_some_and(|u| same_url(u, url)))
}

/// The custom preset for an API kind.
pub fn custom_for(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "custom_anthropic",
        _ => "custom_openai",
    }
}
