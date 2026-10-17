//! Current-conditions pane backed by [Open-Meteo](https://open-meteo.com/).
//!
//! Open-Meteo is free and key-less. We ask only for the `current` block —
//! temperature, a WMO weather code, and wind speed — for a single location.
//! The location comes from `TERMLING_WEATHER="lat,lon"`, falling back to San
//! Francisco so the pane is useful out of the box.
//!
//! Like every online pane it never touches the network itself: [`poll_fetch`]
//! hands [`crate::net`] a [`FetchSpec`] when the cached reading is missing or a
//! few minutes stale, and [`on_data`] folds the JSON (or a short error) back in.
//!
//! [`poll_fetch`]: Pane::poll_fetch
//! [`on_data`]: Pane::on_data

use serde::Deserialize;

use super::{line, Pane, PaneKind};
use crate::doodle::Row;
use crate::net::{self, FetchResult, FetchSpec};
use crate::theme::Theme;

/// Frames between refreshes. The dashboard ticks ~10×/second, so this is ~10 min
/// — weather barely moves and Open-Meteo asks callers to be gentle.
const REFRESH_FRAMES: u64 = 10 * 60 * 10;

/// Default location when `TERMLING_WEATHER` is unset: San Francisco.
const DEFAULT_LATLON: (f64, f64) = (37.77, -122.42);

/// A parsed current-conditions reading.
struct Reading {
    temp: f64,
    temp_unit: String,
    code: i64,
    wind: f64,
    wind_unit: String,
}

pub struct Weather {
    lat: f64,
    lon: f64,
    /// Whether `TERMLING_WEATHER` was set (vs. the default) — shown as a hint.
    custom_loc: bool,
    frame: u64,
    /// Frame the in-flight/last fetch began; `None` until the first fetch.
    fetched_at: Option<u64>,
    /// A request is outstanding — don't queue another until it returns.
    pending: bool,
    reading: Option<Reading>,
    error: Option<String>,
}

impl Weather {
    pub fn new() -> Weather {
        let (lat, lon, custom_loc) = parse_latlon();
        Weather {
            lat,
            lon,
            custom_loc,
            frame: 0,
            fetched_at: None,
            pending: false,
            reading: None,
            error: None,
        }
    }

    fn url(&self) -> String {
        format!(
            "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}\
             &current=temperature_2m,weather_code,wind_speed_10m",
            self.lat, self.lon
        )
    }

    fn due(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(f) => self.frame.saturating_sub(f) >= REFRESH_FRAMES,
        }
    }
}

impl Pane for Weather {
    fn title(&self) -> String {
        "weather".into()
    }

    fn tick(&mut self, frame: u64) {
        self.frame = frame;
    }

    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        if !net::online_enabled() || self.pending || !self.due() {
            return None;
        }
        self.pending = true;
        self.fetched_at = Some(self.frame);
        Some(FetchSpec::get(PaneKind::Weather, self.url()))
    }

    fn on_data(&mut self, result: &FetchResult) {
        self.pending = false;
        match &result.body {
            Err(e) => self.error = Some(short(e)),
            Ok(body) => match parse(body) {
                Ok(r) => {
                    self.reading = Some(r);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            },
        }
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(r) = &self.reading {
            rows.push(line(
                format!("{:.1} {}", r.temp, nonempty(&r.temp_unit, "°")),
                theme.accent,
            ));
            rows.push(line(weather_text(r.code), theme.gem));
            rows.push(line(
                format!("wind {:.1} {}", r.wind, nonempty(&r.wind_unit, "km/h")),
                theme.dim,
            ));
        } else if let Some(e) = &self.error {
            rows.push(line("couldn't fetch:", theme.bad));
            rows.push(line(e.clone(), theme.dim));
        } else if !net::online_enabled() {
            rows.push(line("offline", theme.dim));
            rows.push(line("set TERMLING_ONLINE=1", theme.dim));
        } else {
            rows.push(line("loading…", theme.dim));
        }
        rows.push(line("", theme.dim));
        let loc = format!("{:.2},{:.2}", self.lat, self.lon);
        if self.custom_loc {
            rows.push(line(loc, theme.dim));
        } else {
            rows.push(line(format!("{loc} (set TERMLING_WEATHER)"), theme.dim));
        }
        rows
    }
}

/// Resolve the location from `TERMLING_WEATHER="lat,lon"`, else the default.
/// The bool is whether a (valid) custom value was found.
fn parse_latlon() -> (f64, f64, bool) {
    if let Ok(v) = std::env::var("TERMLING_WEATHER") {
        if let Some((a, b)) = v.split_once(',') {
            if let (Ok(lat), Ok(lon)) = (a.trim().parse(), b.trim().parse()) {
                return (lat, lon, true);
            }
        }
    }
    (DEFAULT_LATLON.0, DEFAULT_LATLON.1, false)
}

#[derive(Deserialize)]
struct OmResponse {
    current: OmCurrent,
    #[serde(default)]
    current_units: OmUnits,
}

#[derive(Deserialize, Default)]
struct OmUnits {
    #[serde(default)]
    temperature_2m: String,
    #[serde(default)]
    wind_speed_10m: String,
}

#[derive(Deserialize)]
struct OmCurrent {
    temperature_2m: f64,
    #[serde(default)]
    weather_code: i64,
    #[serde(default)]
    wind_speed_10m: f64,
}

fn parse(body: &str) -> Result<Reading, String> {
    let r: OmResponse = serde_json::from_str(body).map_err(|e| short(&e.to_string()))?;
    Ok(Reading {
        temp: r.current.temperature_2m,
        temp_unit: r.current_units.temperature_2m,
        code: r.current.weather_code,
        wind: r.current.wind_speed_10m,
        wind_unit: r.current_units.wind_speed_10m,
    })
}

fn nonempty<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.is_empty() {
        fallback
    } else {
        s
    }
}

/// Collapse a possibly-multi-line, possibly-long error into one short line fit
/// for a narrow tile.
fn short(e: &str) -> String {
    let first = e.trim().lines().next().unwrap_or("").trim();
    first.chars().take(56).collect()
}

/// WMO weather-interpretation code → short label.
fn weather_text(code: i64) -> &'static str {
    match code {
        0 => "clear sky",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 | 63 | 65 => "rain",
        66 | 67 => "freezing rain",
        71 | 73 | 75 => "snow",
        77 => "snow grains",
        80..=82 => "rain showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm, hail",
        _ => "—",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    const SAMPLE: &str = r#"{"latitude":37.76,"longitude":-122.4,
        "current_units":{"temperature_2m":"°C","wind_speed_10m":"km/h"},
        "current":{"time":"2024-01-01T12:00","temperature_2m":14.2,
        "weather_code":3,"wind_speed_10m":11.5}}"#;

    fn text(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parses_current_block() {
        let r = parse(SAMPLE).expect("parse");
        assert_eq!(r.temp, 14.2);
        assert_eq!(r.code, 3);
        assert_eq!(r.wind, 11.5);
        assert_eq!(r.temp_unit, "°C");
    }

    #[test]
    fn renders_reading_with_conditions() {
        let mut w = Weather::new();
        w.on_data(&FetchResult {
            pane: PaneKind::Weather,
            body: Ok(SAMPLE.into()),
        });
        let out = text(&w.render(40, 12, &THEMES[0]));
        assert!(out.contains("14.2"), "temp: {out}");
        assert!(out.contains("overcast"), "conditions: {out}");
        assert!(out.contains("wind 11.5"), "wind: {out}");
    }

    #[test]
    fn surfaces_fetch_errors() {
        let mut w = Weather::new();
        w.on_data(&FetchResult {
            pane: PaneKind::Weather,
            body: Err("dns error".into()),
        });
        let out = text(&w.render(40, 12, &THEMES[0]));
        assert!(out.contains("dns error"), "error: {out}");
    }

    #[test]
    fn weather_codes_map_to_labels() {
        assert_eq!(weather_text(0), "clear sky");
        assert_eq!(weather_text(95), "thunderstorm");
        assert_eq!(weather_text(1234), "—");
    }
}
