//! Questions and fields that ask for a secret (rich_plan §4.4). A chat is
//! the wrong place for a password or a key whatever the model says it is
//! for, so such a question is an error and such a form field is refused.
//!
//! This is a word list, so it guesses; it errs towards refusing. The text
//! is read the way a person sees it: characters that show nothing cannot
//! split a word, and letters that look like ASCII ones count as those. A
//! table of look-alikes is never complete, so in a word that mixes ASCII
//! letters with others, the others match any letter.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

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
    let seen: Vec<String> = as_seen(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(read_word)
        .collect();
    if seen.iter().any(|w| same(w, "PIN")) {
        return true;
    }
    asks(&words(&seen, false)) || asks(&words(&seen, true))
}

fn asks(w: &[String]) -> bool {
    if let [only] = w
        && same(only, "pin")
    {
        return true;
    }
    w.iter().any(|x| WORDS.iter().any(|s| same(x, s)))
        || PHRASES.iter().any(|p| has_phrase(w, p))
        || w.windows(2)
            .any(|p| TOKEN_KINDS.iter().any(|k| same(&p[0], k)) && is_word(&p[1], "token"))
        || AMBIGUOUS.iter().any(|a| bare(w, a))
}

/// Is `a.word` there with no neighbour that makes it harmless? Only the
/// neighbour as written makes it harmless: a letter read as any letter
/// counts towards asking, never towards an excuse.
fn bare(w: &[String], a: &Ambiguous) -> bool {
    let numeric = |x: &String| a.numbers && x.chars().any(|c| c.is_ascii_digit());
    let exact = |x: &str, word: &str| x == word || x.strip_suffix('s') == Some(word);
    (0..w.len()).filter(|&i| is_word(&w[i], a.word)).any(|i| {
        let prev = i.checked_sub(1).map(|j| &w[j]);
        let next = w.get(i + 1);
        let harmless = prev.is_some_and(|p| a.prev.contains(&p.as_str()) || numeric(p))
            || next.is_some_and(|n| a.next.iter().any(|x| exact(n, x)) || numeric(n));
        !harmless
    })
}

fn has_phrase(w: &[String], phrase: &[&str]) -> bool {
    let last = phrase.len() - 1;
    w.windows(phrase.len()).any(|win| {
        win.iter()
            .zip(phrase)
            .enumerate()
            .all(|(i, (a, b))| if i == last { is_word(a, b) } else { same(a, b) })
    })
}

/// A letter outside ASCII with no look-alike, in a word that also has
/// ASCII letters: it reads as any letter. Words are split on everything
/// that is not alphanumeric, so no word holds a real `*`.
const ANY: char = '*';

/// `a` reads as `word`: the same letters, `ANY` standing for any one.
fn same(a: &str, word: &str) -> bool {
    a.chars().count() == word.chars().count()
        && a.chars().zip(word.chars()).all(|(x, y)| x == y || x == ANY)
}

/// `a` reads as `word` or its plural.
fn is_word(a: &str, word: &str) -> bool {
    same(a, word)
        || a.char_indices()
            .last()
            .is_some_and(|(i, c)| (c == 's' || c == ANY) && same(&a[..i], word))
}

/// `s` as a person reads it. Compatibility forms (fullwidth, mathematical
/// and circled letters) and letters with marks become their base letters
/// (NFKD, then the marks go), and characters that draw nothing are dropped
/// rather than read as a gap, so "Pass\u{AD}word" is one word.
fn as_seen(s: &str) -> String {
    s.nfkd().filter(|&c| !invisible(c)).collect()
}

/// Draws nothing of its own: the hidden set, combining marks (they change
/// the letter before them) and the rest of the default-ignorable code
/// points (those that are not marks: soft hyphen, Hangul fillers, the
/// Mongolian vowel separator, shorthand format controls) and the
/// interlinear annotation controls.
fn invisible(c: char) -> bool {
    is_hidden(c)
        || is_combining_mark(c)
        || matches!(c,
            '\u{00AD}'
            | '\u{115F}' | '\u{1160}' | '\u{3164}' | '\u{FFA0}'
            | '\u{180E}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}')
}

/// One word as read: look-alikes become the ASCII letter they look like
/// (keeping case), and in a word that also has ASCII letters any other
/// letter outside ASCII is `ANY`, so "Pæssword" asks whatever the table
/// misses. A word with no ASCII letter is another script's word: only its
/// look-alikes change, or "Привет" could read as any six-letter word.
fn read_word(w: &str) -> String {
    let mixed = w.chars().any(|c| c.is_ascii_alphabetic());
    w.chars()
        .map(|c| match look_alike(c) {
            l if l.is_ascii() || !mixed || !l.is_alphabetic() => l,
            _ => ANY,
        })
        .collect()
}

/// Letters that look like an ASCII one but do not decompose to it:
/// Cyrillic, Greek and Armenian look-alikes, IPA letters, small capitals
/// and Latin letters with a stroke.
fn look_alike(c: char) -> char {
    match c {
        'А' | 'Α' => 'A',
        'а' | 'α' | 'ɑ' | 'ᴀ' => 'a',
        'В' | 'Β' => 'B',
        'ʙ' | 'ƀ' => 'b',
        'С' | 'Ϲ' => 'C',
        'с' | 'ϲ' | 'ᴄ' => 'c',
        'Đ' => 'D',
        'ԁ' | 'đ' | 'ᴅ' => 'd',
        'Е' | 'Ε' => 'E',
        'е' | 'ε' | 'ɛ' | 'ᴇ' => 'e',
        'ꜰ' => 'f',
        'ɡ' | 'ց' | 'ɢ' => 'g',
        'Н' | 'Η' | 'Ħ' => 'H',
        'һ' | 'հ' | 'ħ' | 'ʜ' => 'h',
        'І' | 'Ι' | 'Ӏ' => 'I',
        'і' | 'ι' | 'ı' | 'ɩ' | 'ɪ' => 'i',
        'Ј' => 'J',
        'ј' | 'ϳ' | 'ȷ' | 'ᴊ' => 'j',
        'К' | 'Κ' => 'K',
        'к' | 'κ' | 'ᴋ' => 'k',
        'Ł' => 'L',
        'ӏ' | 'ł' | 'ʟ' => 'l',
        'М' | 'Μ' => 'M',
        'ᴍ' => 'm',
        'Ν' => 'N',
        'ո' | 'ɴ' => 'n',
        'О' | 'Ο' | 'Օ' | 'Ø' => 'O',
        'о' | 'ο' | 'օ' | 'ø' | 'ᴏ' => 'o',
        'Р' | 'Ρ' => 'P',
        'р' | 'ρ' | 'ᴘ' => 'p',
        'ԛ' | 'զ' | 'ꞯ' => 'q',
        'ʀ' => 'r',
        'Ѕ' => 'S',
        'ѕ' | 'ꜱ' => 's',
        'Т' | 'Τ' | 'Ŧ' => 'T',
        'ŧ' | 'ᴛ' => 't',
        'Ս' => 'U',
        'υ' | 'ս' | 'ᴜ' => 'u',
        'ν' | 'ᴠ' => 'v',
        'ԝ' | 'ѡ' | 'ω' | 'ᴡ' => 'w',
        'Х' | 'Χ' => 'X',
        'х' | 'χ' => 'x',
        'У' | 'Υ' | 'Ү' => 'Y',
        'у' | 'ү' | 'γ' | 'ʏ' => 'y',
        'Ζ' => 'Z',
        'ᴢ' => 'z',
        _ => c,
    }
}

/// Lowercase words; with `camel`, a capital after a small letter starts one.
fn words(seen: &[String], camel: bool) -> Vec<String> {
    let mut out = Vec::new();
    for w in seen {
        let mut cur = String::new();
        let mut lower = false;
        for c in w.chars() {
            if camel && lower && c.is_uppercase() && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            lower = c.is_lowercase();
            cur.extend(c.to_lowercase());
        }
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
            // Marks of every block, now read as marks (not a list of ranges).
            "Pass\u{17B4}word",
            "Pass\u{180B}word",
            "Pass\u{1AB0}word",
            "Pass\u{1DC0}word",
            "Pass\u{20D0}word",
            "Pass\u{FE20}word",
            "Pass\u{E0100}word",
            // Default-ignorable code points outside the format-character ranges.
            "Pass\u{1D173}word",
            "Pass\u{FFF0}word",
            "Pass\u{E0080}word",
            "Pass\u{E01F0}word",
            "Pass\u{206A}word",
            "Pass\u{1BCA0}word",
            // Latin letters with marks beyond Latin-1, decomposed or not.
            "Pāssword",
            "Paśsword",
            "GitHub tōken",
            "Crēdentials",
            "ṗāśśŵōŕď",
            "Pæssword",
            // IPA, Armenian, small capitals and circled letters.
            "Passw\u{585}rd",
            "P\u{251}ssword",
            "\u{1D18}\u{1D00}\u{A731}\u{A731}\u{1D21}\u{1D0F}\u{280}\u{1D05}",
            "Cr\u{435}d\u{435}nti\u{251}ls",
            "Ⓟⓐⓢⓢⓦⓞⓡⓓ",
            "P\u{12A}N",
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
            // A letter with no ASCII look-alike reads as any letter only in
            // a word that also has ASCII letters, so other scripts and
            // ordinary accented words pass.
            "Größe und Maß",
            "Wrocław, Łódź or Tromsø",
            "Þingvellir",
            "Şanlıurfa",
            "Œuvre",
            "Mật khẩu của bạn",
            "Kayıt tarihi",
            "Nøgle",
            "Секрет",
            "東京 station",
            "Sört key",
        ] {
            assert!(!secret_like(s), "{s}");
        }
    }
}
