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
    let args = ["init", "--template=", "-b", "main", "-q"];
    let mut c = hardened(&git_program());
    c.args(args).current_dir(dir);
    result(&args, exec(c, None, GIT_TIMEOUT).await?)?;
    first_commit(dir).await
}

/// One empty commit by the node's identity on the branch `HEAD` names, in
/// `dir` (a repository the node made or cloned in its staging folder).
pub(crate) async fn first_commit(dir: &Path) -> Result<(), String> {
    let args = [
        "commit",
        "--allow-empty",
        "--no-verify",
        "--no-gpg-sign",
        "-q",
        "-m",
        FIRST_COMMIT,
    ];
    let mut c = hardened(&git_program());
    c.args(args).current_dir(dir);
    result(&args, exec(c, None, GIT_TIMEOUT).await?)?;
    Ok(())
}

/// What a freshly cloned repository's `HEAD` is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Head {
    /// It names a commit.
    Commit,
    /// The repository has no refs at all (an empty repository).
    Empty,
    /// It has refs, but `HEAD` names no commit.
    Dangling,
}

/// [`Head`] of the repository in `dir`.
pub(crate) async fn head_of(dir: &Path) -> Result<Head, String> {
    let mut c = hardened(&git_program());
    c.args(["rev-parse", "--verify", "-q", "HEAD^{commit}"])
        .current_dir(dir);
    if exec(c, None, GIT_TIMEOUT).await?.status.success() {
        return Ok(Head::Commit);
    }
    let args = ["for-each-ref", "--count=1", "--format=%(refname)"];
    let mut c = hardened(&git_program());
    c.args(args).current_dir(dir);
    let refs = result(&args, exec(c, None, GIT_TIMEOUT).await?)?;
    Ok(if refs.is_empty() {
        Head::Empty
    } else {
        Head::Dangling
    })
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
