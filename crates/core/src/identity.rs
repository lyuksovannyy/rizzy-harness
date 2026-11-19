//! The identity an agent's `git commit` carries (PLAN.md T87, README
//! "What an agent's commits are signed with").
//!
//! The owner's decision (2026-11-19): commits made by agents carry the
//! person's own name and e-mail, not one fixed harness identity. The master
//! sends the chat owner's login, display name and the e-mail they set in
//! their profile with every turn ([`CommitIdentity`]); the node validates
//! them again ([`CommitIdentity::resolve`]: a master is not trusted to send
//! a clean value) and puts the result into a git configuration file of its
//! own for the agent's commands (`tools::sandbox`).
//!
//! The e-mail is personal data: [`CommitIdentity`] and [`Identity`] print it
//! as `<hidden>` in `Debug`, so no log line or panic message that formats a
//! command carries it. Events and the audit log never take it.

use serde::{Deserialize, Serialize};

/// The domain of the address commits carry when the person set no e-mail:
/// `.invalid` is reserved (RFC 6761), so it can never be a real mailbox.
pub const NOREPLY_DOMAIN: &str = "users.noreply.rizzy.invalid";
/// The longest e-mail, in bytes (RFC 5321's path limit).
pub const MAX_EMAIL: usize = 254;
/// The longest local part (RFC 5321).
const MAX_LOCAL: usize = 64;
/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 128;
/// The longest login the node accepts, in bytes.
pub const MAX_LOGIN: usize = 64;

/// What the master tells a node about the chat owner, for the commits of
/// every turn of the chat. Every field is checked again by the node.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitIdentity {
    /// The account's login (user name). Builds the no-reply address and is
    /// the name when there is no display name.
    pub login: String,
    /// The display name as the account has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The e-mail the person set "for commits"; none when they did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl std::fmt::Debug for CommitIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitIdentity")
            .field("login", &self.login)
            .field("name", &self.name)
            .field("email", &self.email.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// A resolved identity: what `GIT_AUTHOR_*` and `GIT_COMMITTER_*` would be.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("name", &self.name)
            .field("email", &"<hidden>")
            .finish()
    }
}

fn atext(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// Whether `s` is an e-mail address commits may carry, or why not (a
/// reason that never repeats the value). Narrower
/// than RFC 5322 on purpose: the value goes into a git configuration file
/// and into commit headers, so nothing that could end a line, a quotation
/// or a header is allowed. ASCII only for now (no internationalised
/// addresses): 3 to [`MAX_EMAIL`] bytes, exactly one `@`, a local part of
/// letters, digits and `. _ % + -` (no leading, trailing or doubled dot),
/// a domain of dot-separated labels of letters, digits and `-` with at
/// least two labels (none starting or ending with `-`).
pub fn check_email(s: &str) -> Result<(), String> {
    let bad = |why: &str| Err(why.to_string());
    if s.len() > MAX_EMAIL {
        return bad(&format!("longer than {MAX_EMAIL} characters"));
    }
    if let Some(c) = s.chars().find(|c| !c.is_ascii_graphic()) {
        return bad(&format!(
            "it holds a space, control or non-ASCII character (U+{:04X})",
            c as u32
        ));
    }
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return bad("it needs exactly one @");
    };
    if local.is_empty() || local.len() > MAX_LOCAL {
        return bad("the part before @ is empty or longer than 64 characters");
    }
    if !local.chars().all(atext)
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return bad("the part before @ may hold letters, digits and . _ % + - only");
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2
        || labels.iter().any(|l| {
            l.is_empty()
                || l.starts_with('-')
                || l.ends_with('-')
                || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    {
        return bad("the part after @ must be a domain such as example.org");
    }
    Ok(())
}

/// `s` as a commit name, or `None` when it is not one: 1 to
/// [`MAX_NAME_CHARS`] characters after trimming, non-ASCII letters allowed,
/// but no control character, no `<`, `>`, `"` or `\` (they are special in a
/// commit header and in the configuration file), no line or paragraph
/// separator and no zero-width or bidirectional-override character (a name
/// that reads as another name).
pub fn clean_name(s: &str) -> Option<String> {
    let t = s.trim();
    let n = t.chars().count();
    let bad = |c: char| {
        c.is_control()
            || matches!(c, '<' | '>' | '"' | '\\')
            || matches!(
                c,
                '\u{2028}'
                    | '\u{2029}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}'
            )
    };
    ((1..=MAX_NAME_CHARS).contains(&n) && !t.chars().any(bad)).then(|| t.to_string())
}

/// `login` as the part of the no-reply address before `@`: letters, digits
/// and `. _ -`, at most [`MAX_LOGIN`] bytes, with no dot at either end;
/// anything else is dropped. `user` when nothing is left.
pub fn clean_login(login: &str) -> String {
    let kept: String = login
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(MAX_LOGIN)
        .collect();
    let kept = kept.trim_matches('.').replace("..", ".");
    if kept.is_empty() { "user".into() } else { kept }
}

impl CommitIdentity {
    /// The identity commits carry: the display name (the login when there is
    /// none or it does not pass), the e-mail when it passes
    /// ([`check_email`]), else `<login>@`[`NOREPLY_DOMAIN`]. Never fails: a
    /// value that does not pass is replaced, not repaired.
    pub fn resolve(&self) -> Identity {
        let login = clean_login(&self.login);
        let name = self
            .name
            .as_deref()
            .and_then(clean_name)
            .or_else(|| clean_name(&login))
            .unwrap_or_else(|| "user".into());
        let email = self
            .email
            .as_deref()
            .filter(|e| check_email(e).is_ok())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{login}@{NOREPLY_DOMAIN}"));
        Identity { name, email }
    }
}

impl Identity {
    /// What a chat nobody owns (the API token's) commits as.
    pub fn fallback() -> Self {
        Self {
            name: "Harness agent".into(),
            email: format!("agent@{NOREPLY_DOMAIN}"),
        }
    }

    /// The git configuration file that gives this identity as a default
    /// (`GIT_CONFIG_SYSTEM`, the lowest-precedence level: a global,
    /// repository or `-c` setting wins). Both values passed [`clean_name`]
    /// and [`check_email`], which exclude every character that needs
    /// escaping in a quoted value.
    pub fn gitconfig(&self) -> String {
        format!(
            "# The harness's default commit identity (the chat owner's). Any identity you\n\
             # configure yourself, in the repository or in `git -c`, wins over it.\n\
             [user]\n\tname = \"{}\"\n\temail = \"{}\"\n",
            self.name, self.email
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_addresses_are_refused() {
        let long = format!("{}@example.org", "a".repeat(300));
        let mut cases: Vec<(&str, bool)> = vec![
            ("alex@example.org", true),
            ("alex.b+commits@mail.example.org", true),
            ("a@b.co", true),
            ("alex@example.org\nHost: x", false),
            ("alex@example.org\r", false),
            ("alex@exa mple.org", false),
            (" alex@example.org", false),
            ("Alex <alex@example.org>", false),
            ("<alex@example.org>", false),
            ("alex@example.org>", false),
            ("a@b@example.org", false),
            ("alex", false),
            ("alex@", false),
            ("@example.org", false),
            ("alex@localhost", false),
            ("alex@example..org", false),
            ("alex@-example.org", false),
            ("alex@example.org.", false),
            (".alex@example.org", false),
            ("al..ex@example.org", false),
            ("alex\u{0}@example.org", false),
            ("alex\u{7f}@example.org", false),
            ("alëx@example.org", false),
            ("alex@exämple.org", false),
            ("a\"b@example.org", false),
            ("a\\b@example.org", false),
            ("a;b@example.org", false),
            ("a#b@example.org", false),
            ("a$(id)@example.org", false),
            ("a`id`@example.org", false),
            ("", false),
        ];
        cases.push((&long, false));
        for (s, ok) in cases {
            assert_eq!(check_email(s).is_ok(), ok, "{s:?}");
        }
        let longest = format!("{}@{}.org", "a".repeat(64), "b".repeat(180));
        assert!(longest.len() <= MAX_EMAIL);
        assert!(check_email(&longest).is_ok());
        let over = format!("{}@{}.org", "a".repeat(64), "b".repeat(190));
        assert!(over.len() > MAX_EMAIL && check_email(&over).is_err());
    }

    #[test]
    fn names_may_be_non_ascii_but_never_break_a_header() {
        for ok in ["Alex Example", "Zoë Łukasiewicz", "李小龍", "  Alex  "] {
            assert!(clean_name(ok).is_some(), "{ok:?}");
        }
        assert_eq!(clean_name("  Alex  ").as_deref(), Some("Alex"));
        let long = "x".repeat(129);
        for bad in [
            "",
            "   ",
            "Alex\nBob",
            "Alex\tBob",
            "Alex <a@b.c>",
            "Alex>",
            "A\"lex",
            "A\\lex",
            "Alex\u{2028}Bob",
            "Alex\u{202E}Bob",
            "Alex\u{200B}",
            "Alex\u{0}",
            long.as_str(),
        ] {
            assert!(clean_name(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn resolve_falls_back_instead_of_passing_a_bad_value() {
        let id = |login: &str, name: Option<&str>, email: Option<&str>| {
            CommitIdentity {
                login: login.into(),
                name: name.map(Into::into),
                email: email.map(Into::into),
            }
            .resolve()
        };
        let r = id("alex", Some("Alex Example"), Some("alex@example.org"));
        assert_eq!(
            (r.name.as_str(), r.email.as_str()),
            ("Alex Example", "alex@example.org")
        );
        let r = id("alex", None, None);
        assert_eq!(r.name, "alex");
        assert_eq!(r.email, format!("alex@{NOREPLY_DOMAIN}"));
        // A hostile e-mail and name: the no-reply form and the login.
        let r = id("alex", Some("A\nB"), Some("a@b.org\nX: y"));
        assert_eq!(r.name, "alex");
        assert_eq!(r.email, format!("alex@{NOREPLY_DOMAIN}"));
        // A hostile login cannot reach the no-reply address either.
        let r = id("al\nex <x>@", None, None);
        assert_eq!(r.email, format!("alex@{NOREPLY_DOMAIN}"));
        let r = id("\n", None, None);
        assert_eq!(r.email, format!("user@{NOREPLY_DOMAIN}"));
        assert!(check_email(&r.email).is_ok());
        assert!(check_email(&Identity::fallback().email).is_ok());
    }

    #[test]
    fn debug_never_prints_the_address() {
        let c = CommitIdentity {
            login: "alex".into(),
            name: Some("Alex".into()),
            email: Some("canary-7731@example.org".into()),
        };
        assert!(!format!("{c:?}").contains("canary-7731"));
        assert!(!format!("{:?}", c.resolve()).contains("canary-7731"));
    }
}
