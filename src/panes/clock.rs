//! A local date/time + month-calendar pane. Zero-network, no new deps.
//!
//! Everything is computed from [`std::time::SystemTime`] by hand: the Unix
//! timestamp is shifted by the machine's UTC offset and converted to a civil
//! year/month/day/hour/minute/second with the standard days-since-epoch
//! algorithm (Howard Hinnant's `civil_from_days`). No `chrono`, no crates.
//!
//! The local UTC offset is read once, cheaply and off the render thread, by
//! shelling out to `date +%z` (a plain process spawn, *not* a network call, the
//! same trick [`super::system`] uses). Until it lands — or if `date` is missing —
//! the pane shows UTC and labels it so, so it is always correct about what it's
//! showing and never panics.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::bg::Bg;
use crate::doodle::{Row, Seg};
use crate::panes::line;
use crate::theme::Theme;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The clock tile. Holds the machine's UTC offset (read once off-thread); the
/// time itself is recomputed from the system clock on every [`render`].
///
/// [`render`]: super::Pane::render
pub struct ClockPane {
    /// Local offset from UTC, in seconds (e.g. `-7*3600`). `0` until known.
    offset_secs: i32,
    /// Whether the offset read has landed. Until then we display UTC.
    offset_known: bool,
    /// Off-thread one-shot read of `date +%z`; its result lands on a later tick.
    /// `Some(secs)` on success, `None` when `date` was missing or unparseable.
    bg: Bg<Option<i32>>,
}

impl ClockPane {
    /// Build the pane and kick off the UTC-offset read off-thread, so building it
    /// (which `App::new` does eagerly) never blocks startup on `date`. Until the
    /// result lands, the clock renders in UTC.
    pub fn new() -> ClockPane {
        let bg = Bg::new();
        bg.trigger(read_offset);
        ClockPane {
            offset_secs: 0,
            offset_known: false,
            bg,
        }
    }
}

impl Default for ClockPane {
    fn default() -> Self {
        ClockPane::new()
    }
}

impl super::Pane for ClockPane {
    fn title(&self) -> String {
        "clock".into()
    }

    fn tick(&mut self, _frame: u64) {
        // Fold in the offset read, once, when it lands. A landed-but-`None` result
        // means `date` failed — leave the pane on UTC rather than ever panicking.
        if let Some(Some(secs)) = self.bg.take() {
            self.offset_secs = secs;
            self.offset_known = true;
        }
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        // Seconds since the Unix epoch, shifted into local time.
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let local = unix + self.offset_secs as i64;

        let days = local.div_euclid(86_400);
        let tod = local.rem_euclid(86_400);
        let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
        let (year, month, today) = civil_from_days(days);
        let wd = (days + 4).rem_euclid(7) as usize; // 0 = Sunday

        let mut rows = vec![
            line(
                format!("{} {year:04}-{month:02}-{today:02}", WEEKDAYS[wd]),
                theme.accent,
            ),
            line(format!("{hh:02}:{mm:02}:{ss:02}"), theme.bar),
            line(zone_label(self.offset_known, self.offset_secs), theme.dim),
            line("", theme.dim),
            line(
                format!("{} {year}", MONTHS[(month as usize - 1).min(11)]),
                theme.accent,
            ),
            line("Su Mo Tu We Th Fr Sa", theme.dim),
        ];

        // The month grid, week by week, today highlighted in the bar color.
        let first_wd = (days_from_civil(year, month, 1) + 4).rem_euclid(7) as usize;
        let dim = days_in_month(year, month);
        let mut week: Vec<Seg> = Vec::with_capacity(7);
        // Leading blanks before the 1st.
        for _ in 0..first_wd {
            week.push(Seg {
                text: "   ".into(),
                color: theme.dim,
            });
        }
        for day in 1..=dim {
            let color = if day == today { theme.bar } else { theme.dim };
            week.push(Seg {
                text: format!("{day:>2} "),
                color,
            });
            if week.len() == 7 {
                rows.push(std::mem::take(&mut week));
            }
        }
        if !week.is_empty() {
            rows.push(week);
        }
        rows
    }
}

/// Read the machine's UTC offset in seconds via `date +%z` (e.g. `-0700`). Any
/// failure (missing binary, odd output) yields `None` so the pane stays on UTC.
fn read_offset() -> Option<i32> {
    let out = Command::new("date").arg("+%z").output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_offset(String::from_utf8_lossy(&out.stdout).trim())
}

/// Parse a `±HHMM` zone offset (as `date +%z` prints) into seconds.
fn parse_offset(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    if b.len() < 5 {
        return None;
    }
    let sign: i32 = match b[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i32 = s.get(1..3)?.parse().ok()?;
    let mm: i32 = s.get(3..5)?.parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

/// A friendly zone label: `UTC+05:30` when the offset is known, else `UTC`.
fn zone_label(known: bool, off: i32) -> String {
    if !known {
        return "UTC".into();
    }
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    format!("UTC{sign}{:02}:{:02}", a / 3600, (a % 3600) / 60)
}

/// Civil date `(year, month, day)` from a signed days-since-Unix-epoch count,
/// via Howard Hinnant's `civil_from_days`. Month is 1..=12, day 1..=31.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Signed days-since-Unix-epoch for a civil date, the inverse of
/// [`civil_from_days`]. Used to find what weekday the 1st of the month falls on.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let (m, d) = (m as i64, d as i64);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (d - 1); // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::Pane;
    use crate::theme::THEMES;

    #[test]
    fn renders_non_empty_without_panicking() {
        let pane = ClockPane::new();
        for theme in THEMES {
            let rows = pane.render(40, 12, theme);
            assert!(!rows.is_empty());
        }
        assert_eq!(pane.title(), "clock");
    }

    #[test]
    fn ticking_folds_in_the_offset_without_panicking() {
        let mut pane = ClockPane::new();
        for f in 0..5 {
            pane.tick(f);
        }
        assert!(!pane.render(40, 12, &THEMES[0]).is_empty());
    }

    #[test]
    fn civil_date_round_trips_through_days() {
        // A handful of known anchors (and their weekdays).
        for &(z, y, m, d) in &[
            (0i64, 1970i64, 1u32, 1u32), // Unix epoch, a Thursday
            (18_628, 2021, 1, 1),        // 2021-01-01, a Friday
            (19_723, 2024, 1, 1),        // 2024-01-01 (leap year), a Monday
            (-1, 1969, 12, 31),
        ] {
            assert_eq!(civil_from_days(z), (y, m, d), "civil_from_days({z})");
            assert_eq!(days_from_civil(y, m, d), z, "days_from_civil({y}-{m}-{d})");
        }
        // Weekday mapping (0 = Sunday): 2021-01-01 was a Friday (5).
        assert_eq!((days_from_civil(2021, 1, 1) + 4).rem_euclid(7), 5);
    }

    #[test]
    fn parses_zone_offsets() {
        assert_eq!(parse_offset("-0700"), Some(-7 * 3600));
        assert_eq!(parse_offset("+0530"), Some(5 * 3600 + 30 * 60));
        assert_eq!(parse_offset("+0000"), Some(0));
        assert_eq!(parse_offset("junk"), None);
        assert_eq!(parse_offset(""), None);
        assert_eq!(zone_label(false, 0), "UTC");
        assert_eq!(zone_label(true, 5 * 3600 + 30 * 60), "UTC+05:30");
        assert_eq!(zone_label(true, -7 * 3600), "UTC-07:00");
    }

    #[test]
    fn days_in_month_handles_leap_years() {
        assert_eq!(days_in_month(2024, 2), 29); // leap
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29); // div by 400
        assert_eq!(days_in_month(1900, 2), 28); // div by 100, not 400
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
    }

    #[test]
    fn highlights_today_in_the_grid() {
        // Render a known offset's grid and confirm today's number appears.
        let pane = ClockPane {
            offset_secs: 0,
            offset_known: true,
            bg: Bg::new(),
        };
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (_, _, today) = civil_from_days(unix.div_euclid(86_400));
        let text: String = pane
            .render(40, 12, &THEMES[0])
            .iter()
            .flat_map(|r| r.iter().map(|s| s.text.clone()))
            .collect();
        assert!(
            text.contains(&format!("{today:>2}")),
            "today in grid: {text}"
        );
    }
}
