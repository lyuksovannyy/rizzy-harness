//! A new project's repository (ADOPTION_PLAN X.26, PLAN.md T77): `git init`
//! and one empty commit in a folder the node itself just made in its own
//! staging folder (`<data>/projects-staging`, which no session ever has in
//! view; `crate::projects` moves it into the projects folder afterwards). The node's hardened git as everywhere else: a
//! cleared environment, no system or global config, no hooks, no template
//! (so no sample hooks are copied), no transport, the node's own identity
//! (`harness <harness@localhost>`), a timeout.

use std::path::Path;

use super::{GIT_TIMEOUT, exec, git_program, hardened, result};

/// The first commit's message.
pub const FIRST_COMMIT: &str = "Start the project";

/// Make `dir` (an empty folder the caller made in the staging folder and
/// holds open; passed as `/proc/self/fd/<n>` so git starts in exactly that
/// folder) a repository on branch `main` with one empty commit.
pub(crate) async fn init_project(dir: &Path) -> Result<(), String> {
    for args in [
        &["init", "--template=", "-b", "main", "-q"][..],
        &[
            "commit",
            "--allow-empty",
            "--no-verify",
            "--no-gpg-sign",
            "-q",
            "-m",
            FIRST_COMMIT,
        ][..],
    ] {
        let mut c = hardened(&git_program());
        c.args(args).current_dir(dir);
        result(args, exec(c, None, GIT_TIMEOUT).await?)?;
    }
    Ok(())
}

/// Whether `dir` (the same folder) holds exactly what [`init_project`]
/// makes: one commit, by the node's identity, with the first commit's
/// message.
pub(crate) async fn check_first_commit(dir: &Path) -> Result<(), String> {
    let mut c = hardened(&git_program());
    let args = ["log", "--format=%an <%ae>|%s|%P", "HEAD"];
    c.args(args).current_dir(dir);
    let out = result(&args, exec(c, None, GIT_TIMEOUT).await?)?;
    let want = format!("harness <harness@localhost>|{FIRST_COMMIT}|");
    if out != want {
        return Err(format!("the new repository is not the one made: {out:?}"));
    }
    Ok(())
}

/// Tests: the git program this test thread's calls run (`None`: `git`).
#[cfg(test)]
pub(crate) fn use_test_git(p: Option<std::path::PathBuf>) {
    super::TEST_GIT.with(|g| *g.borrow_mut() = p);
}
