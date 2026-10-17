//! A local machine-vitals pane: load average, uptime, and memory use.
//!
//! Everything is read locally — no network, no new crates. The source differs by
//! OS, chosen with `cfg(target_os)`:
//!
//!   * **macOS** shells out (a plain process spawn, not a network call) to
//!     `uptime` for the load averages, `sysctl` for the boot time, and
//!     `sysctl`/`vm_stat` for memory.
//!   * **Linux** reads `/proc/loadavg`, `/proc/uptime`, and `/proc/meminfo`.
//!   * Any other OS simply reports everything as unavailable.
//!
//! Whatever a platform can't supply shows as "—". Probing is cheap but not free,
//! so the pane refreshes at most once every [`REFRESH_TICKS`] ticks.

use crate::doodle::{Row, Seg};
use crate::theme::Theme;

/// Re-read vitals at most this often (in dashboard ticks).
const REFRESH_TICKS: u64 = 20;

/// A snapshot of machine vitals; every field is optional so a partial read still
/// renders ("—" stands in for anything we couldn't get).
#[derive(Clone, Default)]
struct SysInfo {
    /// 1 / 5 / 15-minute load averages.
    load: Option<(f32, f32, f32)>,
    /// Seconds since boot.
    uptime_s: Option<u64>,
    /// Total physical memory, in kibibytes.
    mem_total_kb: Option<u64>,
    /// Approximate memory in use, in kibibytes.
    mem_used_kb: Option<u64>,
}

/// The vitals tile. Holds a cached [`SysInfo`] refreshed on a tick cadence.
pub struct SystemPane {
    info: SysInfo,
    next_refresh: u64,
}

impl SystemPane {
    /// Build with an initial reading so the tile is never blank on first paint.
    pub fn new() -> SystemPane {
        SystemPane {
            info: collect(),
            next_refresh: REFRESH_TICKS,
        }
    }

    fn refresh(&mut self) {
        self.info = collect();
    }
}

impl Default for SystemPane {
    fn default() -> Self {
        SystemPane::new()
    }
}

impl super::Pane for SystemPane {
    fn title(&self) -> String {
        "system".into()
    }

    fn tick(&mut self, frame: u64) {
        if frame >= self.next_refresh {
            self.refresh();
            self.next_refresh = frame.saturating_add(REFRESH_TICKS);
        }
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let i = &self.info;

        let load = match i.load {
            Some((a, b, c)) => format!("{a:.2} {b:.2} {c:.2}"),
            None => "—".into(),
        };
        let uptime = i.uptime_s.map(human_uptime).unwrap_or_else(|| "—".into());
        let mem = match (i.mem_used_kb, i.mem_total_kb) {
            (Some(u), Some(t)) => format!("{} / {}", human_kb(u), human_kb(t)),
            (None, Some(t)) => format!("— / {}", human_kb(t)),
            _ => "—".into(),
        };

        vec![
            labeled("load", load, theme),
            labeled("up", uptime, theme),
            labeled("mem", mem, theme),
        ]
    }
}

/// A "label  value" row: dim label, accented value.
fn labeled(label: &str, value: String, theme: &Theme) -> Row {
    vec![
        Seg {
            text: format!("{label:<5}"),
            color: theme.dim,
        },
        Seg {
            text: value,
            color: theme.accent,
        },
    ]
}

/// Compact uptime: `3d 2h`, `2h 14m`, or `14m`.
fn human_uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// Kibibytes as a compact human size: `512M`, `15.6G`.
fn human_kb(kb: u64) -> String {
    let mb = kb as f64 / 1024.0;
    if mb < 1024.0 {
        format!("{:.0}M", mb.round())
    } else {
        format!("{:.1}G", mb / 1024.0)
    }
}

// --- platform readers --------------------------------------------------------

#[cfg(target_os = "linux")]
fn collect() -> SysInfo {
    let mut info = SysInfo::default();

    if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
        let mut f = s.split_whitespace();
        if let (Some(a), Some(b), Some(c)) = (f.next(), f.next(), f.next()) {
            if let (Ok(a), Ok(b), Ok(c)) = (a.parse(), b.parse(), c.parse()) {
                info.load = Some((a, b, c));
            }
        }
    }

    if let Ok(s) = std::fs::read_to_string("/proc/uptime") {
        if let Some(first) = s.split_whitespace().next() {
            if let Ok(secs) = first.parse::<f64>() {
                info.uptime_s = Some(secs as u64);
            }
        }
    }

    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        let kv = |key: &str| -> Option<u64> {
            s.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        };
        let total = kv("MemTotal:");
        // MemAvailable is the kernel's own estimate of usable-without-swapping
        // memory; used ≈ total − available.
        let avail = kv("MemAvailable:");
        info.mem_total_kb = total;
        if let (Some(t), Some(a)) = (total, avail) {
            info.mem_used_kb = Some(t.saturating_sub(a));
        }
    }

    info
}

#[cfg(target_os = "macos")]
fn collect() -> SysInfo {
    use std::process::Command;

    let run = |cmd: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    let mut info = SysInfo::default();

    // Load averages from `uptime`: "... load averages: 1.73 1.89 2.01".
    if let Some(up) = run("uptime", &[]) {
        if let Some(idx) = up.find("load average") {
            let tail = &up[idx..];
            let nums: Vec<f32> = tail
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse().ok())
                .collect();
            if nums.len() >= 3 {
                info.load = Some((nums[0], nums[1], nums[2]));
            }
        }
    }

    // Uptime from the boot time: `sysctl -n kern.boottime` → "{ sec = 1700000000, ...".
    if let Some(bt) = run("sysctl", &["-n", "kern.boottime"]) {
        if let Some(sec) = bt
            .split("sec =")
            .nth(1)
            .and_then(|t| {
                t.split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
            })
            .and_then(|s| s.parse::<u64>().ok())
        {
            if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                info.uptime_s = Some(now.as_secs().saturating_sub(sec));
            }
        }
    }

    // Total memory from `sysctl -n hw.memsize` (bytes → KiB).
    if let Some(total_b) = run("sysctl", &["-n", "hw.memsize"]).and_then(|s| s.parse::<u64>().ok())
    {
        info.mem_total_kb = Some(total_b / 1024);

        // Used ≈ total − (free + inactive + speculative) pages, from `vm_stat`.
        if let Some(vm) = run("vm_stat", &[]) {
            let page = vm
                .lines()
                .next()
                .and_then(|l| l.split("page size of").nth(1))
                .and_then(|t| t.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(4096);
            let pages = |label: &str| -> u64 {
                vm.lines()
                    .find(|l| l.starts_with(label))
                    .and_then(|l| l.rsplit(':').next())
                    .map(|v| v.trim().trim_end_matches('.'))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let freeish =
                pages("Pages free") + pages("Pages inactive") + pages("Pages speculative");
            let free_kb = freeish.saturating_mul(page) / 1024;
            info.mem_used_kb = Some((total_b / 1024).saturating_sub(free_kb));
        }
    }

    info
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn collect() -> SysInfo {
    // Unsupported OS: everything renders as "—".
    SysInfo::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::Pane;
    use crate::theme::THEMES;

    #[test]
    fn renders_non_empty_without_panicking() {
        let pane = SystemPane::new();
        for theme in THEMES {
            let rows = pane.render(40, 12, theme);
            assert!(!rows.is_empty());
        }
    }

    #[test]
    fn empty_info_shows_dashes_and_still_renders() {
        let pane = SystemPane {
            info: SysInfo::default(),
            next_refresh: REFRESH_TICKS,
        };
        let rows = pane.render(40, 12, &THEMES[0]);
        assert_eq!(rows.len(), 3);
        // Every value cell should be the "—" placeholder when nothing is known.
        let text: String = rows
            .iter()
            .flat_map(|r| r.iter().map(|s| s.text.clone()))
            .collect();
        assert!(text.contains('—'));
        assert_eq!(pane.title(), "system");
    }

    #[test]
    fn ticking_refreshes_on_cadence_without_panicking() {
        let mut pane = SystemPane::new();
        for f in 0..(REFRESH_TICKS * 2 + 1) {
            pane.tick(f);
        }
        assert!(!pane.render(40, 12, &THEMES[0]).is_empty());
    }

    #[test]
    fn human_helpers_are_sane() {
        assert_eq!(human_uptime(90), "1m");
        assert_eq!(human_uptime(3_700), "1h 1m");
        assert_eq!(human_uptime(180_000), "2d 2h");
        assert_eq!(human_kb(512 * 1024), "512M");
        assert_eq!(human_kb(16 * 1024 * 1024), "16.0G");
    }
}
