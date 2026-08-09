//! Small formatting helpers shared by the CLI and the TUI.
//!
//! Dates are computed locally instead of pulling a calendar crate: the
//! workspace only ever needs a UTC `YYYY-MM-DD HH:MM` stamp and a coarse
//! "time ago" label.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rust_i18n::t;

use crate::error::{Error, Result};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;

/// Writes through a sibling temporary file so a crash cannot truncate the file
/// being replaced.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let directory = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let temporary = directory.join(format!(".{name}.openspectation.tmp"));

    std::fs::write(&temporary, contents).map_err(|source| Error::io(&temporary, source))?;
    std::fs::rename(&temporary, path).map_err(|source| {
        let _ = std::fs::remove_file(&temporary);
        Error::io(path, source)
    })
}

/// Formats a byte count using binary multiples.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// UTC timestamp as `YYYY-MM-DD HH:MM`.
pub fn format_timestamp(time: Option<SystemTime>) -> String {
    let Some(seconds) = epoch_seconds(time) else {
        return t!("time.never").to_string();
    };

    let days = seconds.div_euclid(SECONDS_PER_DAY as i64);
    let rest = seconds.rem_euclid(SECONDS_PER_DAY as i64);
    let (year, month, day) = civil_from_days(days);
    let hour = rest / SECONDS_PER_HOUR as i64;
    let minute = (rest % SECONDS_PER_HOUR as i64) / SECONDS_PER_MINUTE as i64;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// UTC calendar date as `YYYY-MM-DD`.
pub fn format_date(time: Option<SystemTime>) -> String {
    let Some(seconds) = epoch_seconds(time) else {
        return t!("time.never").to_string();
    };
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY as i64));
    format!("{year:04}-{month:02}-{day:02}")
}

/// UTC wall clock as `HH:MM`.
pub fn format_clock(time: Option<SystemTime>) -> String {
    let Some(seconds) = epoch_seconds(time) else {
        return "--:--".to_string();
    };
    let rest = seconds.rem_euclid(SECONDS_PER_DAY as i64);
    let hour = rest / SECONDS_PER_HOUR as i64;
    let minute = (rest % SECONDS_PER_HOUR as i64) / SECONDS_PER_MINUTE as i64;
    format!("{hour:02}:{minute:02}")
}

/// A span of time as a coarse localized label such as `2 h` or `3 d`.
///
/// Shorter than [`format_relative`] and without its "ago": these label how long
/// something took, not how long ago it was.
pub fn format_duration(seconds: u64) -> String {
    if seconds < SECONDS_PER_MINUTE {
        t!("time.seconds", count = seconds).to_string()
    } else if seconds < SECONDS_PER_HOUR {
        t!("time.minutes", count = seconds / SECONDS_PER_MINUTE).to_string()
    } else if seconds < SECONDS_PER_DAY {
        t!("time.hours", count = seconds / SECONDS_PER_HOUR).to_string()
    } else {
        t!("time.days", count = seconds / SECONDS_PER_DAY).to_string()
    }
}

/// Localized coarse age such as "3 h ago", falling back to an absolute date.
pub fn format_relative(time: Option<SystemTime>) -> String {
    let Some(time) = time else {
        return t!("time.never").to_string();
    };

    let Ok(elapsed) = SystemTime::now().duration_since(time) else {
        // Clock skew or a file stamped in the future: an absolute date is safer.
        return format_date(Some(time));
    };

    let seconds = elapsed.as_secs();
    if seconds < SECONDS_PER_MINUTE {
        t!("time.just_now").to_string()
    } else if seconds < SECONDS_PER_HOUR {
        t!("time.minutes_ago", count = seconds / SECONDS_PER_MINUTE).to_string()
    } else if seconds < SECONDS_PER_DAY {
        t!("time.hours_ago", count = seconds / SECONDS_PER_HOUR).to_string()
    } else if seconds < 30 * SECONDS_PER_DAY {
        t!("time.days_ago", count = seconds / SECONDS_PER_DAY).to_string()
    } else {
        format_date(Some(time))
    }
}

/// A completion ratio split into the run that is done and the track behind it.
///
/// The two are returned apart because they are colored apart; joining them is
/// the caller's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// The completed run, ending in a half cell when the remainder earns one.
    pub filled: String,
    /// What is left of the rail.
    pub track: String,
}

/// Renders `ratio` as a rail `width` cells wide.
///
/// A heavy rule rather than a block: `█` turns a column of finished bars into
/// one undifferentiated slab, and the empty half of `░` carries a texture loud
/// enough to compete with the part that means something. `╸` buys half a cell
/// of precision, which is what keeps 4/7 and 5/7 apart at this size.
///
/// Two rules matter more than the glyphs. The rail only fills completely when
/// the work is actually complete — a bar that reads as done at 97% is a lie
/// worth one cell — and any progress at all claims at least half a cell, so
/// the first task ticked off is visible.
pub fn bar(ratio: f64, width: usize) -> Bar {
    const RAIL: char = '━';
    const HALF: char = '╸';

    if width == 0 {
        return Bar {
            filled: String::new(),
            track: String::new(),
        };
    }

    let ratio = ratio.clamp(0.0, 1.0);
    let exact = ratio * width as f64;
    let mut full = exact.floor() as usize;
    let mut half = exact - full as f64 >= 0.5;

    if ratio >= 1.0 {
        full = width;
        half = false;
    } else {
        // Rounding must never reach the end of the rail while work remains, so
        // the last cell stays track and the half is spent rather than kept.
        if full + usize::from(half) >= width {
            full = width.saturating_sub(1);
            half = false;
        }
        if full == 0 && !half && ratio > 0.0 {
            half = true;
        }
    }

    let mut filled = RAIL.to_string().repeat(full);
    if half {
        filled.push(HALF);
    }

    Bar {
        track: RAIL
            .to_string()
            .repeat(width.saturating_sub(full + usize::from(half))),
        filled,
    }
}

/// Turns `user-authentication` or `user_authentication.md` into `User Authentication`.
pub fn humanize(raw: &str) -> String {
    let cleaned = raw.trim().replace(['-', '_'], " ");
    let mut out = String::with_capacity(cleaned.len());

    for (index, word) in cleaned.split_whitespace().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }

    if out.is_empty() { raw.to_string() } else { out }
}

/// Truncates to `max` characters, appending an ellipsis when it had to cut.
pub fn truncate(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// Pads with spaces on the right up to `width` visible characters.
pub fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + (width - len));
    out.push_str(text);
    out.extend(std::iter::repeat_n(' ', width - len));
    out
}

fn epoch_seconds(time: Option<SystemTime>) -> Option<i64> {
    let time = time?;
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).ok(),
        Err(error) => i64::try_from(error.duration().as_secs())
            .ok()
            .map(|value| -value),
    }
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch to `(y, m, d)`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let month_prime = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32; // [1, 31]
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_binary_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn converts_epoch_days_to_civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn humanizes_file_stems() {
        assert_eq!(humanize("user-authentication"), "User Authentication");
        assert_eq!(humanize("api_gateway"), "Api Gateway");
    }

    #[test]
    fn truncates_on_character_boundaries() {
        assert_eq!(truncate("especificación", 6), "espec…");
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn a_bar_always_spans_its_width() {
        for percent in 0..=100 {
            let bar = bar(f64::from(percent) / 100.0, 10);
            assert_eq!(
                bar.filled.chars().count() + bar.track.chars().count(),
                10,
                "at {percent}%"
            );
        }
    }

    #[test]
    fn only_completed_work_fills_the_rail() {
        assert_eq!(bar(1.0, 8).track, "");
        assert_eq!(bar(1.0, 8).filled.chars().count(), 8);

        // 97% rounds to eight cells and would read as finished.
        let almost = bar(0.97, 8);
        assert_eq!(almost.filled, "━━━━━━━");
        assert_eq!(almost.track, "━");
    }

    #[test]
    fn any_progress_claims_at_least_half_a_cell() {
        assert_eq!(bar(0.0, 6).filled, "");
        assert_eq!(bar(0.01, 6).filled, "╸");
        assert_eq!(bar(0.5, 6).filled, "━━━");
    }

    #[test]
    fn a_zero_width_bar_is_empty_rather_than_a_panic() {
        assert_eq!(
            bar(0.5, 0),
            Bar {
                filled: String::new(),
                track: String::new()
            }
        );
    }

    use std::time::Duration;

    fn at(seconds: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(seconds))
    }

    #[test]
    fn sizes_drop_the_decimal_once_they_reach_three_digits() {
        assert_eq!(format_size(100 * 1024), "100 KiB");
        assert_eq!(format_size(99 * 1024), "99.0 KiB");
        // The unit ladder stops at TiB rather than overflowing past it.
        assert!(format_size(u64::MAX).ends_with(" TiB"));
    }

    #[test]
    fn timestamps_are_rendered_in_utc() {
        assert_eq!(format_timestamp(at(0)), "1970-01-01 00:00");
        assert_eq!(format_timestamp(at(3661)), "1970-01-01 01:01");
        assert_eq!(format_date(at(0)), "1970-01-01");
        assert_eq!(format_clock(at(3661)), "01:01");
        assert_eq!(format_clock(at(86_399)), "23:59");
    }

    #[test]
    fn a_time_before_the_epoch_still_produces_a_date() {
        let before = Some(UNIX_EPOCH - Duration::from_secs(SECONDS_PER_DAY));
        assert_eq!(format_date(before), "1969-12-31");
    }

    #[test]
    fn a_missing_time_is_labelled_rather_than_left_blank() {
        assert!(!format_timestamp(None).is_empty());
        assert!(!format_date(None).is_empty());
        assert_eq!(format_clock(None), "--:--");
        assert!(!format_relative(None).is_empty());
    }

    #[test]
    fn a_duration_picks_the_coarsest_unit_that_fits() {
        // The wording is translated; the magnitude is what this pins down.
        assert!(format_duration(45).contains("45"));
        assert!(format_duration(90).contains('1'));
        assert!(format_duration(2 * SECONDS_PER_HOUR).contains('2'));
        assert!(format_duration(3 * SECONDS_PER_DAY).contains('3'));
    }

    #[test]
    fn a_recent_change_reads_as_an_age_and_an_old_one_as_a_date() {
        let now = SystemTime::now();
        assert!(!format_relative(Some(now)).is_empty());
        assert!(
            format_relative(Some(now - Duration::from_secs(2 * SECONDS_PER_HOUR))).contains('2')
        );
        assert!(
            format_relative(Some(now - Duration::from_secs(3 * SECONDS_PER_DAY))).contains('3')
        );

        let ancient = format_relative(Some(now - Duration::from_secs(400 * SECONDS_PER_DAY)));
        assert_eq!(ancient.chars().filter(|c| *c == '-').count(), 2);
    }

    #[test]
    fn a_file_stamped_in_the_future_falls_back_to_an_absolute_date() {
        // Clock skew between a container and its host is the usual cause.
        let future = SystemTime::now() + Duration::from_secs(SECONDS_PER_DAY);
        assert_eq!(format_relative(Some(future)), format_date(Some(future)));
    }

    #[test]
    fn humanize_keeps_something_to_show_for_degenerate_input() {
        assert_eq!(humanize(""), "");
        assert_eq!(humanize("   "), "   ");
        assert_eq!(humanize("---"), "---");
        assert_eq!(humanize("  spaced  out  "), "Spaced Out");
        assert_eq!(humanize("ñandú-azul"), "Ñandú Azul");
    }

    #[test]
    fn truncating_to_nothing_yields_nothing() {
        assert_eq!(truncate("text", 0), "");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
    }

    #[test]
    fn padding_counts_characters_and_never_shortens() {
        assert_eq!(pad_right("ab", 5), "ab   ");
        assert_eq!(pad_right("abcdef", 3), "abcdef");
        assert_eq!(pad_right("ñandú", 6), "ñandú ");
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temporary_behind() {
        let directory = std::env::temp_dir().join(format!(
            "openspectation-util-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("doc.md");
        std::fs::write(&path, "old").unwrap();

        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");

        let leftovers: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(
            leftovers.len(),
            1,
            "temporary file left behind: {leftovers:?}"
        );
    }

    #[test]
    fn a_failed_rename_reports_the_target_and_cleans_up_after_itself() {
        let directory = std::env::temp_dir().join(format!(
            "openspectation-util-{}-{}",
            std::process::id(),
            line!()
        ));
        // Renaming a file onto a directory fails, which is the branch under test.
        let target = directory.join("occupied");
        std::fs::create_dir_all(&target).unwrap();

        let result = write_atomic(&target, "new");
        let leftovers = std::fs::read_dir(&directory).unwrap().count();
        let _ = std::fs::remove_dir_all(&directory);

        match result {
            Err(Error::Io { path, .. }) => assert_eq!(path, target),
            other => panic!("expected Io, got {other:?}"),
        }
        assert_eq!(leftovers, 1, "temporary file survived the failure");
    }
}
