//! PROTOTYPE (measurement only, branch `exp/shell-write-nudge`): does a
//! `bash` command author a file in the chat's folder from literal text
//! (`cat > f <<EOF`, `echo x > f`, `printf … >> f`, `tee f <<EOF`)? Such a
//! command is not run; the agent is told to use `write` or `edit`.
//!
//! A program's own output redirected to a file (`make > log`, `grep x > out`,
//! `cargo test 2>&1 | tee log`) is not literal text and passes.

use std::path::Path;

/// The first file `command` writes from literal text, if any.
pub fn literal_file_write(command: &str, cwd: &Path) -> Option<String> {
    let segs = split(command);
    for (i, seg) in segs.iter().enumerate() {
        let prev_literal = i > 0 && seg.piped && {
            let p = &segs[i - 1];
            matches!(p.program.as_deref(), Some("echo" | "printf"))
                || (p.program.as_deref() == Some("cat") && p.heredoc)
        };
        let Some(prog) = seg.program.as_deref() else {
            continue;
        };
        let targets: Vec<&String> = match prog {
            // Literal text by itself: any output redirection is an authored file.
            "echo" | "printf" => seg.redirects.iter().collect(),
            // Only with a heredoc: `cat f > g` copies, `cat > g` reads a pipe.
            "cat" if seg.heredoc => seg.redirects.iter().collect(),
            // `tee f` writes what it is given: literal only after a heredoc or an echo.
            "tee" if seg.heredoc || prev_literal => {
                seg.args.iter().filter(|a| !a.starts_with('-')).collect()
            }
            _ => continue,
        };
        for t in targets {
            if inside_folder(t, cwd) {
                return Some(t.clone());
            }
        }
    }
    None
}

/// Relative paths and absolute ones under `cwd`; not `/dev/*`, `/tmp`,
/// `$TMPDIR`, other variables, `~`, or a file-descriptor target (`&1`).
fn inside_folder(path: &str, cwd: &Path) -> bool {
    if path.is_empty() || path.starts_with('&') || path.starts_with('$') || path.starts_with('~') {
        return false;
    }
    if path.starts_with('/') {
        return !path.starts_with("/dev/")
            && path != "/dev/null"
            && Path::new(path).starts_with(cwd)
            && !path.starts_with("/tmp");
    }
    true
}

#[derive(Default, Debug)]
struct Seg {
    program: Option<String>,
    args: Vec<String>,
    redirects: Vec<String>,
    heredoc: bool,
    /// The previous segment ended with `|`.
    piped: bool,
}

/// A small shell reader: quotes, `;`, `&&`, `||`, `|`, `&`, newlines, here-documents
/// (their bodies are skipped), output redirections. Not a full parser: anything it
/// does not understand is left as a word.
#[allow(unused_assignments)]
fn split(cmd: &str) -> Vec<Seg> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut segs: Vec<Seg> = Vec::new();
    let mut cur = Seg::default();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut pending_redirect = false;
    let mut pending_heredocs: Vec<(String, bool)> = Vec::new();
    let mut piped_next = false;
    let mut i = 0;

    macro_rules! end_word {
        () => {
            if in_word {
                let w = std::mem::take(&mut word);
                in_word = false;
                if pending_redirect {
                    cur.redirects.push(w);
                    pending_redirect = false;
                } else {
                    words.push(w);
                }
            }
        };
    }
    macro_rules! end_seg {
        ($pipe:expr) => {{
            end_word!();
            let mut it = std::mem::take(&mut words).into_iter().peekable();
            // Leading `VAR=value` words are not the program.
            while it.peek().is_some_and(|w| is_assignment(w)) {
                it.next();
            }
            cur.program = it
                .next()
                .map(|p| p.rsplit('/').next().unwrap_or(&p).to_string());
            cur.args = it.collect();
            cur.piped = piped_next;
            piped_next = $pipe;
            segs.push(std::mem::take(&mut cur));
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    word.push(chars[i]);
                    i += 1;
                }
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    word.push(chars[i]);
                    i += 1;
                }
            }
            '\\' if i + 1 < chars.len() => {
                in_word = true;
                i += 1;
                word.push(chars[i]);
            }
            ' ' | '\t' => end_word!(),
            '\n' => {
                end_seg!(false);
                // Here-document bodies start on the next line.
                for (delim, strip) in std::mem::take(&mut pending_heredocs) {
                    i += 1;
                    loop {
                        let start = i;
                        while i < chars.len() && chars[i] != '\n' {
                            i += 1;
                        }
                        let line: String = chars[start..i].iter().collect();
                        let line = if strip {
                            line.trim_start_matches('\t')
                        } else {
                            &line
                        };
                        if line == delim || i >= chars.len() {
                            break;
                        }
                        i += 1;
                    }
                }
            }
            ';' => end_seg!(false),
            // `2>&1`, `>&2`: the `&` belongs to the redirection's target.
            '&' if pending_redirect && !in_word => {
                in_word = true;
                word.push('&');
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                i += 1;
                end_seg!(false);
            }
            '&' if chars.get(i + 1) == Some(&'>') => {
                // `&>file`
                end_word!();
                i += 1;
                if chars.get(i + 1) == Some(&'>') {
                    i += 1;
                }
                pending_redirect = true;
            }
            '&' => end_seg!(false),
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    i += 1;
                    end_seg!(false);
                } else {
                    end_seg!(true);
                }
            }
            '<' if chars.get(i + 1) == Some(&'<') && chars.get(i + 2) != Some(&'<') => {
                // Here-document: the delimiter word follows.
                end_word!();
                i += 2;
                let strip = chars.get(i) == Some(&'-');
                if strip {
                    i += 1;
                }
                while chars.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
                    i += 1;
                }
                let mut delim = String::new();
                let mut quote: Option<char> = None;
                while i < chars.len() {
                    let d = chars[i];
                    match (quote, d) {
                        (None, '\'' | '"') => quote = Some(d),
                        (Some(q), _) if d == q => quote = None,
                        (None, ' ' | '\t' | '\n' | ';' | '|' | '&' | ')') => break,
                        _ => delim.push(d),
                    }
                    i += 1;
                }
                i -= 1;
                cur.heredoc = true;
                pending_heredocs.push((delim, strip));
            }
            '>' => {
                // A leading file-descriptor digit is part of the operator (`2>`).
                if in_word && word.chars().all(|d| d.is_ascii_digit()) {
                    word.clear();
                    in_word = false;
                }
                end_word!();
                if chars.get(i + 1) == Some(&'>') {
                    i += 1;
                }
                pending_redirect = true;
            }
            _ => {
                in_word = true;
                word.push(c);
            }
        }
        i += 1;
    }
    end_seg!(false);
    segs
}

fn is_assignment(w: &str) -> bool {
    match w.split_once('=') {
        Some((k, _)) => {
            !k.is_empty()
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !k.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(c: &str) -> Option<String> {
        literal_file_write(c, Path::new("/work/repo"))
    }

    /// The command Claude sent for the two-file task (round 1, appendix).
    #[test]
    fn the_two_file_command_is_refused() {
        let c = "cat > config.json <<'EOF'\n{\n  \"name\": \"calc\",\n  \"timeout\": 30\n}\nEOF\n\
                 python3 - <<'EOF'\np='README.md'\nopen(p,'w').write('x')\nEOF\ngit diff";
        assert_eq!(w(c).as_deref(), Some("config.json"));
    }

    #[test]
    fn literal_text_into_a_file_is_refused() {
        assert_eq!(w("echo hi > notes.txt").as_deref(), Some("notes.txt"));
        assert_eq!(w("printf 'a\\nb\\n' >> log.md").as_deref(), Some("log.md"));
        assert_eq!(
            w("cd sub && cat > a/b.py <<EOF\nx\nEOF").as_deref(),
            Some("a/b.py")
        );
        assert_eq!(w("tee f.txt <<EOF\nx\nEOF").as_deref(), Some("f.txt"));
        assert_eq!(
            w("cat <<EOF | tee out.md\nx\nEOF").as_deref(),
            Some("out.md")
        );
        assert_eq!(
            w("echo x > /work/repo/src/a.rs").as_deref(),
            Some("/work/repo/src/a.rs")
        );
        // Part of a list.
        assert_eq!(w("mkdir d; echo 1 > d/f").as_deref(), Some("d/f"));
    }

    #[test]
    fn a_programs_own_output_passes() {
        for c in [
            "make > build.log 2>&1",
            "grep -r x . > out.txt",
            "cargo test 2>&1 | tee log",
            "echo hi",
            "cat file",
            "cat a b > c",
            "cat <<EOF | python3\nprint(1)\nEOF",
            "cat > f",
            "echo x > /dev/null",
            "echo x > $TMPDIR/a",
            "echo x > /tmp/a",
            "echo x 2>&1",
            "ls > /dev/null 2>&1; echo done",
            "python3 - <<'EOF'\nopen('a','w').write('x')\nEOF",
            "git diff > /work/other/p.diff",
        ] {
            assert_eq!(w(c), None, "{c}");
        }
    }

    /// A heredoc body is text, not commands: a `;` or `> f` inside it is not read.
    #[test]
    fn a_heredoc_body_is_skipped() {
        assert_eq!(w("python3 - <<EOF\nprint('a'); echo y > z\nEOF"), None);
    }
}
