//! Questions and fields that ask for a secret (rich_plan §4.4). A chat is
//! the wrong place for a password or a key whatever the model says it is
//! for, so such a question is an error and such a form field is refused.

/// Words that ask for a secret wherever they appear.
const WORDS: &[&str] = &[
    "password",
    "passwords",
    "passphrase",
    "passcode",
    "apikey",
    "secret",
    "secrets",
    "credential",
    "credentials",
    "mnemonic",
    "otp",
    "2fa",
    "mfa",
    "cvv",
    "cvc",
    "iban",
    "ssn",
];

/// Phrases, matched as whole words; the last word may be plural.
const PHRASES: &[&[&str]] = &[
    &["pin", "code"],
    &["pin", "number"],
    &["api", "key"],
    &["access", "key"],
    &["private", "key"],
    &["ssh", "key"],
    &["seed", "phrase"],
    &["recovery", "phrase"],
    &["recovery", "code"],
    &["backup", "code"],
    &["one", "time", "code"],
    &["verification", "code"],
    &["card", "number"],
    &["credit", "card"],
    &["social", "security"],
];

/// `<word> token(s)`: kinds of token that are credentials. "Max tokens" and
/// "token budget" are not.
const TOKEN_KINDS: &[&str] = &[
    "access", "auth", "bearer", "refresh", "api", "session", "github", "gitlab", "slack", "bot",
    "personal", "oauth", "jwt", "secret", "your",
];

/// Does `s` (a question, a label, help text, a field id) ask for a secret?
/// Lowercased, with `_` and `-` read as spaces, matched by whole words; a
/// camelCase string is also read split (`accessToken`). `pin` and `token`
/// count alone only as the whole string ("PIN", "Token"), or `PIN` written
/// in capitals, so "Pin this chat" and "Token budget" pass.
pub fn secret_like(s: &str) -> bool {
    if s.split(|c: char| !c.is_alphanumeric()).any(|w| w == "PIN") {
        return true;
    }
    asks(&words(s, false)) || asks(&words(s, true))
}

fn asks(w: &[String]) -> bool {
    if let [only] = w
        && (only == "pin" || only == "token")
    {
        return true;
    }
    w.iter().any(|x| WORDS.contains(&x.as_str()))
        || PHRASES.iter().any(|p| has_phrase(w, p))
        || w.windows(2)
            .any(|p| TOKEN_KINDS.contains(&p[0].as_str()) && is_word(&p[1], "token"))
}

fn has_phrase(w: &[String], phrase: &[&str]) -> bool {
    let last = phrase.len() - 1;
    w.windows(phrase.len()).any(|win| {
        win.iter()
            .zip(phrase)
            .enumerate()
            .all(|(i, (a, b))| if i == last { is_word(a, b) } else { a == b })
    })
}

/// `a` is `word` or its plural.
fn is_word(a: &str, word: &str) -> bool {
    a == word || a.strip_suffix('s') == Some(word)
}

/// Lowercase words; with `camel`, a capital after a small letter starts one.
fn words(s: &str, camel: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            lower = false;
            continue;
        }
        if camel && lower && c.is_uppercase() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        lower = c.is_lowercase();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_secrets() {
        for s in [
            "Password",
            "Your password, please",
            "API key",
            "api_key",
            "apiKey",
            "GitHub token",
            "Access tokens",
            "accessToken",
            "Seed phrase",
            "CVV",
            "IBAN",
            "2FA code",
            "One-time code",
            "PIN",
            "Enter your PIN",
            "pin",
            "Token",
            "Private keys",
            "Credit card number",
            "Social security number",
            "Secret",
        ] {
            assert!(secret_like(s), "{s}");
        }
    }

    #[test]
    fn ordinary_words_pass() {
        for s in [
            "Max tokens",
            "Token budget",
            "Pin this chat",
            "Secretary",
            "Pinned messages",
            "Keyboard layout",
            "Passport country",
            "Which database should I set up?",
            "Travellers",
            "Destination",
            "Name",
            "Spinning",
        ] {
            assert!(!secret_like(s), "{s}");
        }
    }
}
