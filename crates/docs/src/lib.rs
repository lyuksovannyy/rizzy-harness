//! Word and Excel files made from the model's own text (PLAN.md T97,
//! ADOPTION_PLAN X.56). No docx or xlsx crate and no `zip` crate: a new file
//! is a handful of XML parts, and only one of them varies, so each part is a
//! string template over a fixed style set, written by [`zip::ZipWriter`] (the
//! twin of the bounded reader in `harness-attached`). The input here is what
//! the model wrote, never someone's file: every string is escaped and
//! stripped of what XML 1.0 forbids ([`xml`]), every size is counted before
//! it is built ([`limits`]), and nothing is fetched or embedded.
//!
//! * [`docx::build`]: Markdown to `.docx` (headings 1 to 4, paragraphs, bold,
//!   italic, strike, inline code, links to http, https and mailto, nested
//!   lists, tables, block quotes, code blocks, a rule as a page break). Raw
//!   HTML is dropped and images are not embedded: the result says so.
//! * [`xlsx::build`]: a JSON sheet spec to `.xlsx` (typed cells, a closed
//!   list of formats, real dates, formulas from a deny-list-checked field,
//!   frozen bold header, autofilter, widths).
//! * [`name::file_name`]: the name of the file, its extension forced.
//!
//! The editing tools that follow build on the same writer and helpers.

pub mod docx;
pub mod formula;
pub mod limits;
pub mod name;
pub mod notes;
pub mod xlsx;
pub mod xml;
pub mod zip;

#[cfg(test)]
mod tests;

/// A refusal: one sentence for the model, with no text of its own input in
/// it beyond what the caller chooses to quote.
pub type Result<T> = std::result::Result<T, String>;

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-11-23T10:04:05Z` for a Unix time in seconds: the creation time the
/// package properties carry.
pub fn iso_utc(unix_s: i64) -> String {
    let days = unix_s.div_euclid(86_400);
    let rest = unix_s.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// The wall clock as Unix seconds (0 when it is before 1970).
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
