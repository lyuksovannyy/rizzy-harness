//! Questions and fields that ask for a secret (rich_plan §4.4). A chat is
//! the wrong place for a password or a key whatever the model says it is
//! for, so such a question is an error and such a form field is refused.
//!
//! This is a word list, so it guesses; it errs towards refusing. The text
//! is read the way a person sees it: characters that show nothing cannot
//! split a word, and letters that look like ASCII ones count as those.

use crate::text::is_hidden;

/// Words that ask for a secret wherever they appear.
const WORDS: &[&str] = &[
    "password",
    "passwords",
    "passwd",
    "pwd",
    "passphrase",
    "passcode",
    "passkey",
    "passkeys",
    "apikey",
    "keypair",
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

/// Phrases, matched as whole words; the last word may be plural. A phrase
/// counts whatever follows it, so "API key features" still asks.
const PHRASES: &[&[&str]] = &[
    &["pin", "code"],
    &["pin", "number"],
    &["api", "key"],
    &["access", "key"],
    &["private", "key"],
    &["secret", "key"],
    &["ssh", "key"],
    &["recovery", "key"],
    &["license", "key"],
    &["product", "key"],
    &["seed", "phrase"],
    &["recovery", "phrase"],
    &["recovery", "code"],
    &["backup", "code"],
    &["one", "time", "code"],
    &["verification", "code"],
    &["confirmation", "code"],
    &["security", "code"],
    &["auth", "code"],
    &["authorization", "code"],
    &["authentication", "code"],
    &["access", "code"],
    &["login", "code"],
    &["sign", "in", "code"],
    &["signin", "code"],
    &["session", "cookie"],
    &["auth", "cookie"],
    &["login", "cookie"],
    &["cookie", "value"],
    &["card", "number"],
    &["credit", "card"],
    &["social", "security"],
];

/// `<word> token(s)`: kinds of token that are credentials, whatever
/// follows ("GitHub token budget" still asks).
const TOKEN_KINDS: &[&str] = &[
    "access", "auth", "bearer", "refresh", "api", "session", "github", "gitlab", "slack", "bot",
    "personal", "oauth", "jwt", "secret", "your",
];

/// `key` and `token` ask for a secret on their own ("OpenAI key", "npm
/// token", "Key") unless the word before or after makes them something
/// else: a database or keyboard key, "key points", a count of model tokens.
struct Ambiguous {
    word: &'static str,
    prev: &'static [&'static str],
    next: &'static [&'static str],
    /// A number next to it ("8k tokens", "tokens: 500") makes it harmless.
    numbers: bool,
}

const AMBIGUOUS: &[Ambiguous] = &[
    Ambiguous {
        word: "key",
        prev: &[
            "sort",
            "primary",
            "foreign",
            "partition",
            "composite",
            "unique",
            "cache",
            "lookup",
            "map",
            "dictionary",
            "hot",
            "shortcut",
            "arrow",
            "modifier",
            "music",
            "piano",
        ],
        next: &[
            "point",
            "takeaway",
            "finding",
            "feature",
            "date",
            "metric",
            "result",
            "term",
            "idea",
            "concept",
            "factor",
            "decision",
            "insight",
            "question",
            "step",
            "topic",
            "difference",
            "change",
            "requirement",
            "risk",
            "goal",
            "milestone",
        ],
        numbers: false,
    },
    Ambiguous {
        word: "token",
        prev: &[
            "max",
            "maximum",
            "min",
            "minimum",
            "input",
            "output",
            "total",
            "prompt",
            "completion",
            "context",
            "cached",
            "reasoning",
            "thinking",
            "many",
            "few",
            "more",
            "fewer",
            "less",
            "count",
            "design",
        ],
        next: &[
            "budget", "limit", "count", "usage", "cost", "price", "window", "rate", "length",
            "per", "used", "left", "spent",
        ],
        numbers: true,
    },
];

/// Does `s` (a question, a label, help text, a field id) ask for a secret?
/// Lowercased, with `_` and `-` read as spaces, matched by whole words; a
/// camelCase string is also read split (`accessToken`). `pin` counts alone
/// only as the whole string ("PIN") or written in capitals, so "Pin this
/// chat" passes.
pub fn secret_like(s: &str) -> bool {
    let s = as_seen(s);
    if s.split(|c: char| !c.is_alphanumeric()).any(|w| w == "PIN") {
        return true;
    }
    asks(&words(&s, false)) || asks(&words(&s, true))
}

fn asks(w: &[String]) -> bool {
    if let [only] = w
        && only == "pin"
    {
        return true;
    }
    w.iter().any(|x| WORDS.contains(&x.as_str()))
        || PHRASES.iter().any(|p| has_phrase(w, p))
        || w.windows(2)
            .any(|p| TOKEN_KINDS.contains(&p[0].as_str()) && is_word(&p[1], "token"))
        || AMBIGUOUS.iter().any(|a| bare(w, a))
}

/// Is `a.word` there with no neighbour that makes it harmless?
fn bare(w: &[String], a: &Ambiguous) -> bool {
    let numeric = |x: &String| a.numbers && x.chars().any(|c| c.is_ascii_digit());
    (0..w.len()).filter(|&i| is_word(&w[i], a.word)).any(|i| {
        let prev = i.checked_sub(1).map(|j| &w[j]);
        let next = w.get(i + 1);
        let harmless = prev.is_some_and(|p| a.prev.contains(&p.as_str()) || numeric(p))
            || next.is_some_and(|n| a.next.iter().any(|x| is_word(n, x)) || numeric(n));
        !harmless
    })
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

/// `s` as a person reads it. Characters that draw nothing (the hidden set,
/// soft hyphens, joiners, variation selectors, combining marks) are
/// dropped rather than read as a gap, so "Pass\u{AD}word" is one word; and
/// fullwidth, mathematical, Cyrillic and Greek look-alikes and accented
/// Latin letters become the ASCII letter they look like, keeping case.
fn as_seen(s: &str) -> String {
    s.chars()
        .filter(|&c| !invisible(c))
        .map(look_alike)
        .collect()
}

fn invisible(c: char) -> bool {
    is_hidden(c)
        || matches!(c,
            '\u{00AD}'
            | '\u{034F}'
            | '\u{0300}'..='\u{036F}'
            | '\u{115F}' | '\u{1160}' | '\u{3164}' | '\u{FFA0}'
            | '\u{17B4}' | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{2060}'..='\u{206F}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}')
}

fn look_alike(c: char) -> char {
    let n = c as u32;
    let ascii = |b: u32| char::from_u32(b).unwrap_or(c);
    match n {
        // Fullwidth ASCII.
        0xFF01..=0xFF5E => ascii(n - 0xFEE0),
        // Mathematical letters: 13 styles of A-Z then a-z.
        0x1D400..=0x1D6A3 => {
            let k = (n - 0x1D400) % 52;
            ascii(if k < 26 {
                'A' as u32 + k
            } else {
                'a' as u32 + k - 26
            })
        }
        // Mathematical digits: 5 styles of 0-9.
        0x1D7CE..=0x1D7FF => ascii('0' as u32 + (n - 0x1D7CE) % 10),
        _ => match c {
            'А' | 'Α' | 'À'..='Å' => 'A',
            'а' | 'α' | 'à'..='å' => 'a',
            'В' | 'Β' => 'B',
            'С' | 'Ç' => 'C',
            'с' | 'ç' => 'c',
            'ԁ' => 'd',
            'Е' | 'Ε' | 'È'..='Ë' => 'E',
            'е' | 'ε' | 'è'..='ë' => 'e',
            'Н' | 'Η' => 'H',
            'һ' => 'h',
            'І' | 'Ι' | 'Ì'..='Ï' => 'I',
            'і' | 'ι' | 'ì'..='ï' => 'i',
            'Ј' => 'J',
            'ј' => 'j',
            'К' | 'Κ' => 'K',
            'к' | 'κ' => 'k',
            'ӏ' => 'l',
            'М' | 'Μ' => 'M',
            'Ν' | 'Ñ' => 'N',
            'ñ' => 'n',
            'О' | 'Ο' | 'Ò'..='Ö' => 'O',
            'о' | 'ο' | 'ò'..='ö' => 'o',
            'Р' | 'Ρ' => 'P',
            'р' | 'ρ' => 'p',
            'ԛ' => 'q',
            'Ѕ' => 'S',
            'ѕ' => 's',
            'Т' | 'Τ' => 'T',
            'Ù'..='Ü' => 'U',
            'υ' | 'ù'..='ü' => 'u',
            'ν' => 'v',
            'ԝ' => 'w',
            'Х' | 'Χ' => 'X',
            'х' | 'χ' => 'x',
            'У' | 'Υ' | 'Ý' => 'Y',
            'у' | 'ý' | 'ÿ' => 'y',
            'Ζ' => 'Z',
            _ => c,
        },
    }
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
            // A provider's key or token, or one with no name at all.
            "OpenAI key",
            "Your key",
            "Stripe live key",
            "Key",
            "Keys",
            "License key",
            "Recovery key",
            "API key features",
            "Paste the key from your dashboard",
            "npm token",
            "Hugging Face token",
            "Paste the token",
            "Tokens",
            "GitHub token budget",
            // Other names for passwords and codes.
            "Passwd",
            "pwd",
            "Passkey",
            "Security code",
            "Auth code",
            "Authorization code",
            "Access code",
            "Sign-in code",
            "Session cookie",
        ] {
            assert!(secret_like(s), "{s}");
        }
    }

    #[test]
    fn invisible_characters_and_look_alikes_do_not_hide_a_word() {
        for s in [
            "Pass\u{ad}word",
            "Pass\u{34f}word",
            "Pass\u{fe0f}word",
            "Pass\u{180e}word",
            "Pass\u{200b}word",
            "Pass\u{2063}word",
            "Pas\u{301}sword",
            "P\u{430}ssword",
            "ＰＡＳＳＷＯＲＤ",
            "Your G\u{456}tHub token",
            "\u{1D40F}\u{1D41A}\u{1D42C}\u{1D42C}\u{1D430}\u{1D428}\u{1D42B}\u{1D41D}",
            "\u{420}IN",
            "Pässwörd",
            "\u{391}PI key",
        ] {
            assert!(secret_like(s), "{s:?}");
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
            "Sort key",
            "Primary keys",
            "Foreign key to users",
            "Key points",
            "Key takeaways from the report",
            "Which hot key?",
            "How many tokens should the summary use?",
            "Output tokens",
            "Token limit",
            "8k tokens",
            "Tokens: 500",
            "Keyword",
            "Monkey",
            "Tokenizer",
            "Trip details",
            "Anything else?",
            "Connect the service",
            "Привет, мир",
            "Ｗｈｉｃｈ colour?",
        ] {
            assert!(!secret_like(s), "{s}");
        }
    }
}
