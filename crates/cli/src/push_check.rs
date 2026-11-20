//! `doctor`'s "push from a chat" block (ADOPTION_PLAN X.40).
//!
//! What a host-shell chat needs to run `git push` over SSH or `gh`, said
//! from node.toml and the node's own environment only: nothing is
//! contacted, no key, token or file is read, and a socket is not even
//! looked at. Each line names the node.toml entry that would change it.
//! The decisions are plain functions of [`PushView`], so a few
//! configurations test them all.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use harness_tools::ToolEnv;

/// The tokens `gh` reads from its environment.
const GH_TOKENS: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN"];

/// What the lines are decided from.
pub struct PushView<'a> {
    /// Whether the node offers the host shell (the mode this is about).
    pub host_offered: bool,
    /// The environment a command gets: the core list plus `env_pass`,
    /// minus the names never passed.
    pub env: &'a ToolEnv,
    /// The node process's own environment.
    pub node_var: &'a dyn Fn(&str) -> Option<OsString>,
    /// node.toml's `[sandbox] read` entries, `~/` expanded.
    pub read: &'a [PathBuf],
    /// node.toml's `[sandbox] write` entries, `~/` expanded.
    pub write: &'a [PathBuf],
    /// The node user's home folder.
    pub home: Option<&'a Path>,
}

/// Whether a command may open `p`: `p` lies under one of `entries`
/// (compared as written, and by real path where both exist).
fn covered(p: &Path, entries: &[PathBuf]) -> bool {
    let real = p.canonicalize().ok();
    entries.iter().any(|e| {
        p.starts_with(e)
            || real
                .as_ref()
                .zip(e.canonicalize().ok())
                .is_some_and(|(r, e)| r.starts_with(e))
    })
}

fn passed(v: &PushView, name: &str) -> bool {
    v.env.allows(name)
}

fn set(v: &PushView, name: &str) -> Option<PathBuf> {
    (v.node_var)(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// The lines, each complete without the others.
pub fn lines(v: &PushView) -> Vec<String> {
    if !v.host_offered {
        return vec![
            "this node does not offer the host shell, the mode this check is about \
             (`[execution] host = true`)"
                .into(),
        ];
    }
    let mut out = vec![ssh_agent(v), known_hosts(v), git_config(v)];
    out.extend(gh(v));
    out
}

fn ssh_agent(v: &PushView) -> String {
    let who = "ssh agent";
    let sock = set(v, "SSH_AUTH_SOCK");
    match (passed(v, "SSH_AUTH_SOCK"), sock) {
        (false, None) => format!(
            "{who}: not passed to commands, and the node has no SSH_AUTH_SOCK; \
             `env_pass = [\"SSH_AUTH_SOCK\"]` passes it once the node is started from a \
             shell that has an agent"
        ),
        (false, Some(s)) => format!(
            "{who}: not passed to commands; `env_pass = [\"SSH_AUTH_SOCK\"]` passes {} and \
             lets a command use every key in that agent",
            s.display()
        ),
        (true, None) => format!(
            "{who}: SSH_AUTH_SOCK is passed, but the node has none set (start the node from \
             a shell that has an agent)"
        ),
        (true, Some(s)) if covered(&s, v.write) => format!(
            "{who}: passed and reachable ({}, under a `[sandbox] write` entry): a command can \
             use every key in that agent",
            s.display()
        ),
        (true, Some(s)) if covered(&s, v.read) => format!(
            "{who}: passed, but {} is only under a `[sandbox] read` entry: connecting to a \
             socket needs write; `write = [\"{}\"]` (the socket file itself)",
            s.display(),
            s.display()
        ),
        (true, Some(s)) => format!(
            "{who}: passed, but a command cannot reach {}; `[sandbox] write = [\"{}\"]` (the \
             socket file itself) lets it connect",
            s.display(),
            s.display()
        ),
    }
}

fn known_hosts(v: &PushView) -> String {
    let who = "ssh known_hosts";
    let ssh_command = passed(v, "GIT_SSH_COMMAND") && set(v, "GIT_SSH_COMMAND").is_some();
    let Some(home) = v.home else {
        return format!("{who}: the node user has no home folder; ssh has no known_hosts to read");
    };
    let file = home.join(".ssh/known_hosts");
    let all: Vec<PathBuf> = v.read.iter().chain(v.write).cloned().collect();
    let tail = if ssh_command {
        "; GIT_SSH_COMMAND is passed and may name another file: that one must be granted too"
    } else {
        ""
    };
    if covered(&file, &all) {
        format!("{who}: {} is readable{tail}", file.display())
    } else {
        format!(
            "{who}: {} is not readable, so ssh stops with `Host key verification failed` \
             (a command cannot answer its question); `[sandbox] read = [\"~/.ssh/known_hosts\"]` \
             when the host is already in it{tail}",
            file.display()
        )
    }
}

fn git_config(v: &PushView) -> String {
    let who = "git config";
    let Some(home) = v.home else {
        return format!("{who}: the node user has no home folder");
    };
    let file = home.join(".gitconfig");
    let all: Vec<PathBuf> = v.read.iter().chain(v.write).cloned().collect();
    if covered(&file, &all) {
        format!(
            "{who}: {} is readable: its url rewrites, `core.sshCommand` and [user] apply",
            file.display()
        )
    } else {
        format!(
            "{who}: {} is not readable: its url rewrites and `core.sshCommand` do not apply \
             (the commit identity still comes from your profile); `[sandbox] read = \
             [\"~/.gitconfig\"]` applies them",
            file.display()
        )
    }
}

fn gh(v: &PushView) -> Vec<String> {
    let mut out = Vec::new();
    let tokens: Vec<&str> = GH_TOKENS
        .iter()
        .copied()
        .filter(|n| passed(v, n) && set(v, n).is_some())
        .collect();
    out.push(if tokens.is_empty() {
        "gh token: no token variable reaches commands; `env_pass = [\"GH_TOKEN\"]` with the \
         token in the node's environment lets a command act as you on your forge"
            .to_string()
    } else {
        format!(
            "gh token: {} reaches commands: a command can act as you on your forge",
            tokens.join(", ")
        )
    });
    let all: Vec<PathBuf> = v.read.iter().chain(v.write).cloned().collect();
    let dir = if passed(v, "GH_CONFIG_DIR") {
        set(v, "GH_CONFIG_DIR")
    } else {
        None
    };
    out.push(match (dir, v.home) {
        (Some(d), _) if covered(&d, &all) => {
            format!("gh config: GH_CONFIG_DIR {} is readable", d.display())
        }
        (Some(d), _) => format!(
            "gh config: GH_CONFIG_DIR {} is passed but not readable, so gh stops with \
             `permission denied`; `[sandbox] read = [\"{}\"]`",
            d.display(),
            d.display()
        ),
        (None, Some(h)) if covered(&h.join(".config/gh"), &all) => format!(
            "gh config: {} is readable (its logins are available to commands)",
            h.join(".config/gh").display()
        ),
        (None, _) => "gh config: gh's own folder is not readable and no GH_CONFIG_DIR is \
                      passed: gh starts without a login; `[sandbox] read = [\"~/.config/gh\"]` \
                      gives it yours"
            .to_string(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pass: &[&str]) -> ToolEnv {
        ToolEnv::new(&pass.iter().map(|s| s.to_string()).collect::<Vec<_>>(), []).unwrap()
    }

    fn run(
        host: bool,
        pass: &[&str],
        node: &[(&str, &str)],
        read: &[&str],
        write: &[&str],
    ) -> Vec<String> {
        let e = env(pass);
        let node: Vec<(String, String)> = node
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let get = move |n: &str| {
            node.iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| OsString::from(v))
        };
        let p = |l: &[&str]| l.iter().map(PathBuf::from).collect::<Vec<_>>();
        lines(&PushView {
            host_offered: host,
            env: &e,
            node_var: &get,
            read: &p(read),
            write: &p(write),
            home: Some(Path::new("/home/me")),
        })
    }

    fn line<'a>(l: &'a [String], area: &str) -> &'a str {
        l.iter()
            .find(|s| s.starts_with(area))
            .unwrap_or_else(|| panic!("no {area} line in {l:?}"))
    }

    #[test]
    fn a_node_without_the_host_shell_says_one_line() {
        let l = run(false, &[], &[], &[], &[]);
        assert_eq!(l.len(), 1);
        assert!(l[0].contains("[execution] host = true"));
    }

    #[test]
    fn the_default_node_pushes_nothing_and_each_line_names_its_entry() {
        let l = run(true, &[], &[("SSH_AUTH_SOCK", "/run/a.sock")], &[], &[]);
        let a = line(&l, "ssh agent");
        assert!(a.contains("not passed") && a.contains("env_pass"), "{a}");
        assert!(line(&l, "ssh known_hosts").contains("~/.ssh/known_hosts"));
        assert!(line(&l, "git config").contains("~/.gitconfig"));
        assert!(line(&l, "gh token").contains("GH_TOKEN"));
        assert!(line(&l, "gh config").contains("~/.config/gh"));
    }

    #[test]
    fn the_agent_socket_needs_write_not_read() {
        let sock = [("SSH_AUTH_SOCK", "/run/user/1/agent.sock")];
        let a = |read: &[&str], write: &[&str]| {
            line(
                &run(true, &["SSH_AUTH_SOCK"], &sock, read, write),
                "ssh agent",
            )
            .to_string()
        };
        assert!(a(&[], &[]).contains("cannot reach"));
        let r = a(&["/run/user/1"], &[]);
        assert!(
            r.contains("only under a `[sandbox] read`") && r.contains("needs write"),
            "{r}"
        );
        for w in ["/run/user/1/agent.sock", "/run/user/1"] {
            let ok = a(&[], &[w]);
            assert!(ok.contains("passed and reachable"), "{ok}");
        }
        // Passed, but the node has no agent of its own.
        let none = line(&run(true, &["SSH_AUTH_SOCK"], &[], &[], &[]), "ssh agent").to_string();
        assert!(none.contains("node has none set"), "{none}");
        // Not passed, though the node has one.
        let off = a_not_passed();
        assert!(
            off.contains("not passed") && off.contains("/run/user/1/agent.sock"),
            "{off}"
        );
    }

    fn a_not_passed() -> String {
        let l = run(
            true,
            &[],
            &[("SSH_AUTH_SOCK", "/run/user/1/agent.sock")],
            &[],
            &["/run/user/1"],
        );
        line(&l, "ssh agent").to_string()
    }

    #[test]
    fn known_hosts_and_gitconfig_follow_their_grants() {
        let l = run(true, &[], &[], &["/home/me/.ssh/known_hosts"], &[]);
        assert!(line(&l, "ssh known_hosts").contains("is readable"));
        assert!(line(&l, "git config").contains("not readable"));
        // A whole folder, or a write entry, covers a file under it.
        let l = run(true, &[], &[], &["/home/me/.ssh"], &["/home/me/.gitconfig"]);
        assert!(line(&l, "ssh known_hosts").contains("is readable"));
        assert!(line(&l, "git config").contains("is readable"));
        // A neighbour does not.
        let l = run(true, &[], &[], &["/home/me/.ssh/known_hosts2"], &[]);
        assert!(line(&l, "ssh known_hosts").contains("not readable"));
        // GIT_SSH_COMMAND, passed and set, may name another file.
        let l = run(
            true,
            &["GIT_SSH_COMMAND"],
            &[("GIT_SSH_COMMAND", "ssh -o X=1")],
            &[],
            &[],
        );
        assert!(line(&l, "ssh known_hosts").contains("GIT_SSH_COMMAND is passed"));
    }

    #[test]
    fn gh_lines_follow_the_token_and_the_config_folder() {
        let l = run(true, &["GH_TOKEN"], &[("GH_TOKEN", "x")], &[], &[]);
        let t = line(&l, "gh token");
        assert!(t.contains("GH_TOKEN reaches commands"), "{t}");
        assert!(!t.contains("GITHUB_TOKEN"), "{t}");
        // Passed but not set for the node: nothing reaches a command.
        let l = run(true, &["GH_TOKEN"], &[], &[], &[]);
        assert!(line(&l, "gh token").contains("no token variable"));
        // The value is never printed.
        let l = run(true, &["GH_*"], &[("GH_TOKEN", "ghp_secret0123")], &[], &[]);
        assert!(!l.join("\n").contains("ghp_secret0123"));
        // GH_CONFIG_DIR passed: readable or the `permission denied` case.
        let n = [("GH_CONFIG_DIR", "/opt/gh")];
        let l = run(true, &["GH_CONFIG_DIR"], &n, &[], &[]);
        assert!(line(&l, "gh config").contains("permission denied"));
        let l = run(true, &["GH_CONFIG_DIR"], &n, &["/opt/gh"], &[]);
        assert!(line(&l, "gh config").contains("/opt/gh is readable"));
        // Set for the node but not passed: the default folder counts.
        let l = run(true, &[], &n, &["/home/me/.config/gh"], &[]);
        assert!(line(&l, "gh config").contains("/home/me/.config/gh is readable"));
    }

    #[test]
    fn provider_key_names_are_never_reported_as_passed() {
        // HARNESS_* is refused by the tool environment whatever env_pass says.
        let e = ToolEnv::new(&["GH_TOKEN".to_string()], ["GH_TOKEN".to_string()]).unwrap();
        let get = |_: &str| Some(OsString::from("x"));
        let l = lines(&PushView {
            host_offered: true,
            env: &e,
            node_var: &get,
            read: &[],
            write: &[],
            home: None,
        });
        assert!(line(&l, "gh token").contains("no token variable"), "{l:?}");
    }
}
