//! The providers onboarding and settings offer, in the order they are
//! shown: the admin's own endpoints first, then known platforms. Fixed
//! presets pin their base URL: the server ignores any URL a client sends
//! for them.

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

/// Where the picker lists a preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// An endpoint the admin runs or names (custom, Ollama).
    Own,
    /// A known platform.
    Known,
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
    pub group: Group,
}

const TRUSTED: Warning = Warning {
    kind: WarningKind::TrustedSource,
    text: "The endpoint receives every prompt and file the assistant sends.",
    wait_secs: 0,
};

/// A data-use warning. `wait_secs` is 10 only where the platform always
/// collects or trains on API data; where it depends on the plan, the model
/// or a setting, the warning shows without a wait. Each one cites the
/// policy it rests on (read 2026-11). OpenAI, Anthropic, Z.ai, xAI, Groq
/// and Fireworks do not train on API data unless the customer opts in:
/// developers.openai.com/api/docs/guides/your-data,
/// privacy.claude.com/en/articles/7996868, docs.z.ai/legal-agreement/terms-of-use,
/// docs.x.ai/developers/faq/security, console.groq.com/docs/your-data,
/// docs.fireworks.ai/guides/security_compliance/data_handling.
const fn data_use(kind: WarningKind, text: &'static str, wait_secs: u32) -> Option<Warning> {
    Some(Warning {
        kind,
        text,
        wait_secs,
    })
}

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
        group: Group::Known,
    }
}

pub const PRESETS: &[Preset] = &[
    // Local OpenAI-compatible servers (llama.cpp, vLLM, LM Studio) often
    // take no key, so none is required here.
    Preset {
        base_url: None,
        url_editable: true,
        needs_key: false,
        warning: Some(TRUSTED),
        custom: true,
        group: Group::Own,
        ..fixed("custom_openai", "OpenAI-compatible (custom)", "")
    },
    Preset {
        kind: ProviderKind::Anthropic,
        base_url: None,
        url_editable: true,
        warning: Some(TRUSTED),
        custom: true,
        group: Group::Own,
        ..fixed("custom_anthropic", "Anthropic-compatible (custom)", "")
    },
    Preset {
        url_editable: true,
        needs_key: false,
        group: Group::Own,
        ..fixed("ollama", "Ollama", "http://localhost:11434/v1")
    },
    fixed("openai", "OpenAI (ChatGPT)", "https://api.openai.com/v1"),
    Preset {
        kind: ProviderKind::Anthropic,
        ..fixed(
            "anthropic",
            "Anthropic (Claude)",
            "https://api.anthropic.com/v1",
        )
    },
    Preset {
        // Free tier only: https://aistudio.google.com/docs/terms
        warning: data_use(
            WarningKind::TrainingData,
            "On the free Gemini API tier, Google may use prompts and responses to improve its \
             models, and human reviewers may read them; a billing-enabled project stops this.",
            0,
        ),
        ..fixed(
            "gemini",
            "Google AI Studio (Gemini)",
            "https://generativelanguage.googleapis.com/v1beta/openai",
        )
    },
    Preset {
        // Free plan, opt-out: https://help.mistral.ai/en/articles/347617
        // and https://help.mistral.ai/en/articles/455207
        warning: data_use(
            WarningKind::TrainingData,
            "On Mistral's free Experiment plan, prompts and outputs may be used for training \
             unless you turn this off under Privacy in the Mistral admin console.",
            0,
        ),
        ..fixed("mistral", "Mistral", "https://api.mistral.ai/v1")
    },
    Preset {
        // Always: the privacy policy trains on "User Input", does not exempt
        // the API, and opts out only by email request (the API terms point
        // personal data there): https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html
        // and https://cdn.deepseek.com/policies/en-US/deepseek-open-platform-terms-of-service.html
        warning: data_use(
            WarningKind::TrainingData,
            "DeepSeek's privacy policy lets it use your inputs to train its models; opting out \
             takes an email request to DeepSeek.",
            10,
        ),
        ..fixed("deepseek", "DeepSeek", "https://api.deepseek.com/v1")
    },
    Preset {
        // Always, no opt-out (API Trial Terms 3.3):
        // https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf
        warning: data_use(
            WarningKind::TrainingData,
            "NVIDIA's free API trial terms let NVIDIA collect prompts and outputs to improve its \
             products and AI models, so avoid sending private or confidential content.",
            10,
        ),
        ..fixed(
            "nvidia_nim",
            "NVIDIA NIM",
            "https://integrate.api.nvidia.com/v1",
        )
    },
    Preset {
        // Always, unless agreed otherwise in writing (section 4):
        // https://platform.kimi.ai/docs/agreement/modeluse
        warning: data_use(
            WarningKind::TrainingData,
            "Moonshot's Kimi API terms allow it to use prompts and outputs to improve its \
             services unless you have a separate written agreement.",
            10,
        ),
        ..fixed("moonshot", "Kimi (Moonshot)", "https://api.moonshot.ai/v1")
    },
    fixed("zai", "Z.ai (GLM)", "https://api.z.ai/api/paas/v4"),
    fixed("xai", "xAI (Grok)", "https://api.x.ai/v1"),
    fixed("groq", "Groq", "https://api.groq.com/openai/v1"),
    Preset {
        // Per upstream provider and account setting:
        // https://openrouter.ai/docs/guides/privacy/provider-logging
        // and https://openrouter.ai/docs/guides/privacy/data-collection
        warning: data_use(
            WarningKind::TrainingData,
            "Through OpenRouter, some upstream providers, often on free models, may train on or \
             keep prompts; check the training and retention options in your OpenRouter privacy \
             settings.",
            0,
        ),
        ..fixed("openrouter", "OpenRouter", "https://openrouter.ai/api/v1")
    },
    Preset {
        // Stored by default, may be used for product improvements; training
        // is opt-in: https://docs.together.ai/docs/privacy-and-security
        warning: data_use(
            WarningKind::DataCollection,
            "Together AI stores prompts and responses by default and may use them for product \
             improvements; an organization admin can turn storage off under Privacy settings.",
            0,
        ),
        ..fixed("together", "Together AI", "https://api.together.xyz/v1")
    },
    fixed(
        "fireworks",
        "Fireworks",
        "https://api.fireworks.ai/inference/v1",
    ),
];

/// The built-in scripted provider (not listed: onboarding's "Skip").
pub const DEMO: &str = "demo";

/// Ids a provider cannot be saved under: `/api/admin/providers/<id>` is an
/// action for these (`models/test` for `models`).
pub const RESERVED_IDS: &[&str] = &[DEMO, "test", "models"];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_groups_and_labels() {
        let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            [
                "custom_openai",
                "custom_anthropic",
                "ollama",
                "openai",
                "anthropic",
                "gemini",
                "mistral",
                "deepseek",
                "nvidia_nim",
                "moonshot",
                "zai",
                "xai",
                "groq",
                "openrouter",
                "together",
                "fireworks",
            ]
        );
        let own = PRESETS.iter().take_while(|p| p.group == Group::Own).count();
        assert_eq!(own, 3);
        assert!(PRESETS[own..].iter().all(|p| p.group == Group::Known));
        assert_eq!(get("gemini").unwrap().label, "Google AI Studio (Gemini)");
    }

    /// The policy research of 2026-11: a 10 s wait only where the platform
    /// always uses API data, a plain warning where it depends, none where
    /// it does not.
    #[test]
    fn warnings_match_the_policies() {
        use WarningKind::*;
        let expect: &[(&str, Option<(WarningKind, u32)>)] = &[
            ("custom_openai", Some((TrustedSource, 0))),
            ("custom_anthropic", Some((TrustedSource, 0))),
            ("ollama", None),
            ("openai", None),
            ("anthropic", None),
            ("gemini", Some((TrainingData, 0))),
            ("mistral", Some((TrainingData, 0))),
            ("deepseek", Some((TrainingData, 10))),
            ("nvidia_nim", Some((TrainingData, 10))),
            ("moonshot", Some((TrainingData, 10))),
            ("zai", None),
            ("xai", None),
            ("groq", None),
            ("openrouter", Some((TrainingData, 0))),
            ("together", Some((DataCollection, 0))),
            ("fireworks", None),
        ];
        for (id, want) in expect {
            let got = get(id).unwrap().warning.map(|w| (w.kind, w.wait_secs));
            assert_eq!(got, *want, "{id}");
        }
        assert!(
            get("openrouter")
                .unwrap()
                .warning
                .unwrap()
                .text
                .contains("free models")
        );
    }
}
