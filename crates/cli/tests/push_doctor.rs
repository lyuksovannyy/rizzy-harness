//! `doctor`'s "push from a chat" block (ADOPTION_PLAN X.40), through the
//! real binary: the lines follow node.toml and the process's environment,
//! and name no secret.

use std::path::Path;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// `doctor` with this `node.toml` and a minimal environment: the push lines.
fn push_lines(node_toml: &str, vars: &[(&str, &str)], home: &Path) -> Vec<String> {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("node.toml"), node_toml).unwrap();
    let out = std::process::Command::new(EXE)
        .args(["--data-dir", data.path().to_str().unwrap(), "doctor"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home)
        .envs(vars.iter().copied())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("info  push from a chat, "))
        .map(|l| l["info  push from a chat, ".len()..].to_string())
        .collect()
}

fn area<'a>(l: &'a [String], a: &str) -> &'a str {
    l.iter()
        .find(|s| s.starts_with(a))
        .unwrap_or_else(|| panic!("no {a} line in {l:#?}"))
}

#[test]
fn doctor_says_what_a_host_chat_can_push_with() {
    let home = tempfile::tempdir().unwrap();
    let sock = home.path().join("agent/agent.sock");
    let known = home.path().join(".ssh/known_hosts");
    std::fs::create_dir_all(known.parent().unwrap()).unwrap();
    std::fs::write(&known, "").unwrap();
    let vars = [
        ("SSH_AUTH_SOCK", sock.to_str().unwrap()),
        ("GH_TOKEN", "ghp_never_printed_0123456789"),
    ];

    // The node a new user has: nothing is passed or readable.
    let l = push_lines("[execution]\nhost = true\n", &vars, home.path());
    assert!(area(&l, "ssh agent").contains("not passed"), "{l:#?}");
    assert!(area(&l, "ssh known_hosts").contains("is not readable"));
    assert!(area(&l, "gh token").contains("no token variable"));

    // The lines of the README's recipe.
    let toml = format!(
        "env_pass = [\"SSH_AUTH_SOCK\", \"GH_TOKEN\"]\n[execution]\nhost = true\n\
         [sandbox]\nwrite = [\"{}\"]\nread = [\"~/.ssh/known_hosts\"]\n",
        sock.display()
    );
    let l = push_lines(&toml, &vars, home.path());
    assert!(
        area(&l, "ssh agent").contains("passed and reachable"),
        "{l:#?}"
    );
    assert!(area(&l, "ssh known_hosts").contains("is readable"));
    assert!(area(&l, "gh token").contains("GH_TOKEN reaches commands"));
    assert!(
        !l.join("\n").contains("ghp_never_printed"),
        "the token is never printed"
    );

    // `read` on the socket is not enough.
    let toml = format!(
        "env_pass = [\"SSH_AUTH_SOCK\"]\n[execution]\nhost = true\n[sandbox]\nread = [\"{}\"]\n",
        sock.parent().unwrap().display()
    );
    let l = push_lines(&toml, &vars, home.path());
    assert!(area(&l, "ssh agent").contains("needs write"), "{l:#?}");

    // Without the host shell the block says why it has nothing to check.
    let l = push_lines("[execution]\nhost = false\n", &vars, home.path());
    assert_eq!(l.len(), 1, "{l:#?}");
    assert!(l[0].contains("does not offer the host shell"));
}
