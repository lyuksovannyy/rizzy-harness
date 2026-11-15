//! Model prices: data the server's admin sets, never a built-in table
//! (research round 2, OpenHands: a shipped price table is "stale the moment
//! models change"). Per model, per million tokens, each class optional
//! (`[[providers.models]] price`), and one currency label for the server
//! (`[pricing]`). The UI turns a chat's per-model token sums into an
//! estimate at these prices; nothing here limits or bills anything.

use serde::{Deserialize, Serialize};

/// The most one million tokens of any class may cost (any currency): far
/// above every published rate, low enough that a typo of a few zeros is
/// refused rather than shown as a fortune.
pub const MAX_PRICE: f64 = 10_000.0;

/// `[pricing]`: written only when changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PricingConfig {
    /// The ISO 4217 code every model price is in (`USD`, `EUR`, `JPY`).
    pub currency: String,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            currency: "USD".into(),
        }
    }
}

impl PricingConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `code` as stored (trimmed, upper case) when it has the shape of an ISO
/// 4217 code: three ASCII letters. Whether the browser knows it is the
/// browser's to say (`Intl.NumberFormat` shows the code itself otherwise).
pub fn clean_currency(code: &str) -> Result<String, String> {
    let c = code.trim().to_ascii_uppercase();
    if c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()) {
        Ok(c)
    } else {
        Err(format!(
            "the currency must be a three-letter ISO 4217 code such as USD, EUR or JPY, not {code:?}"
        ))
    }
}

/// One model's prices per million tokens, in the server's currency. A
/// class left out is unknown, never free: a model is priced only for the
/// classes its chats actually used, so an estimate never comes out too low
/// by a missing field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPrice {
    /// Input tokens not read from or written to the provider's cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    /// Input tokens read from the provider's prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input: Option<f64>,
    /// Input tokens written to the provider's prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
}

impl ModelPrice {
    /// Each set price is a finite number from 0 to [`MAX_PRICE`]; the
    /// error names the class (the caller names the model).
    pub fn check(&self) -> Result<(), String> {
        for (name, v) in [
            ("input", self.input),
            ("cached_input", self.cached_input),
            ("cache_write", self.cache_write),
            ("output", self.output),
        ] {
            if let Some(v) = v
                && !(v.is_finite() && (0.0..=MAX_PRICE).contains(&v))
            {
                return Err(format!(
                    "the {name} price must be a number from 0 to {MAX_PRICE} per million tokens, not {v}"
                ));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A model's prices as the model list carries them: with the currency, so
/// the UI needs nothing else to show an estimate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceInfo {
    #[serde(flatten)]
    pub price: ModelPrice,
    pub currency: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_are_checked_per_class() {
        let ok = ModelPrice {
            input: Some(0.0),
            cached_input: Some(0.3),
            cache_write: Some(MAX_PRICE),
            output: None,
        };
        assert!(ok.check().is_ok());
        for bad in [-0.01, f64::NAN, f64::INFINITY, MAX_PRICE * 1.01] {
            let p = ModelPrice {
                output: Some(bad),
                ..Default::default()
            };
            assert!(p.check().unwrap_err().contains("output"), "{bad}");
        }
    }

    #[test]
    fn currencies_are_three_letters() {
        assert_eq!(clean_currency(" eur ").unwrap(), "EUR");
        for bad in ["", "EURO", "E1R", "€", "US"] {
            assert!(clean_currency(bad).is_err(), "{bad:?}");
        }
    }
}
