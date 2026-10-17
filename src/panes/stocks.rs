//! A quotes pane backed by [Stooq](https://stooq.com/)'s light CSV endpoint.
//!
//! Stooq serves a key-less CSV for a comma-separated list of symbols; we ask
//! for `s`ymbol, `d`ate, `t`ime, `o`pen/`h`igh/`l`ow/`c`lose and `v`olume. The
//! watchlist comes from `TERMLING_STOCKS="AAPL,MSFT"`, defaulting to a couple of
//! well-known tickers. We show the last price and the day's change (close minus
//! open), colored up/down.
//!
//! The CSV is trivial, so we parse it by hand rather than pull in a CSV crate.

use super::{line, Pane, PaneKind};
use crate::doodle::{Row, Seg};
use crate::net::{self, FetchSpec};
use crate::theme::Theme;

/// Frames between refreshes (~5 min at the dashboard's ~10 ticks/second). Quotes
/// move, but Stooq is a courtesy endpoint — polling harder would be rude.
const REFRESH_FRAMES: u64 = 5 * 60 * 10;

/// Default watchlist when `TERMLING_STOCKS` is unset.
const DEFAULT_SYMBOLS: &str = "AAPL,MSFT";

/// One parsed quote.
struct Quote {
    symbol: String,
    close: f64,
    change: f64,
    change_pct: f64,
}

pub struct Stocks {
    symbols: Vec<String>,
    custom: bool,
    frame: u64,
    fetched_at: Option<u64>,
    pending: bool,
    quotes: Vec<Quote>,
    error: Option<String>,
}

impl Stocks {
    pub fn new() -> Stocks {
        let (symbols, custom) = parse_symbols();
        Stocks {
            symbols,
            custom,
            frame: 0,
            fetched_at: None,
            pending: false,
            quotes: Vec::new(),
            error: None,
        }
    }

    fn url(&self) -> String {
        // `AAPL,MSFT` -> `aapl.us,msft.us`
        let list = self
            .symbols
            .iter()
            .map(|s| format!("{}.us", s.to_lowercase()))
            .collect::<Vec<_>>()
            .join(",");
        format!("https://stooq.com/q/l/?s={list}&f=sd2t2ohlcv&h&e=csv")
    }

    fn due(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(f) => self.frame.saturating_sub(f) >= REFRESH_FRAMES,
        }
    }
}

impl Pane for Stocks {
    fn title(&self) -> String {
        "stocks".into()
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
        Some(FetchSpec::get(PaneKind::Stocks, self.url()))
    }

    fn on_data(&mut self, result: &crate::net::FetchResult) {
        self.pending = false;
        match &result.body {
            Err(e) => self.error = Some(short(e)),
            Ok(body) => {
                let quotes = parse_csv(body);
                if quotes.is_empty() {
                    self.error = Some("no quotes".into());
                } else {
                    self.quotes = quotes;
                    self.error = None;
                }
            }
        }
    }

    fn render(&self, _width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Vec::new();
        if !self.quotes.is_empty() {
            for q in &self.quotes {
                let color = if q.change >= 0.0 {
                    theme.good
                } else {
                    theme.bad
                };
                let sign = if q.change >= 0.0 { "+" } else { "" };
                rows.push(vec![
                    Seg {
                        text: format!("{:<6}", q.symbol),
                        color: theme.accent,
                    },
                    Seg {
                        text: format!("{:>9.2}  ", q.close),
                        color: theme.dim,
                    },
                    Seg {
                        text: format!("{sign}{:.2} ({sign}{:.2}%)", q.change, q.change_pct),
                        color,
                    },
                ]);
            }
        } else if let Some(e) = &self.error {
            rows.push(line("couldn't fetch:", theme.bad));
            rows.push(line(e.clone(), theme.dim));
        } else if !net::online_enabled() {
            rows.push(line("offline", theme.dim));
            rows.push(line("set TERMLING_ONLINE=1", theme.dim));
        } else {
            rows.push(line("loading…", theme.dim));
        }
        if !self.custom {
            rows.push(line("", theme.dim));
            rows.push(line("set TERMLING_STOCKS", theme.dim));
        }
        rows
    }
}

/// Resolve the watchlist from `TERMLING_STOCKS`, else the default. Bool is
/// whether a custom (non-empty) list was found.
fn parse_symbols() -> (Vec<String>, bool) {
    let (raw, custom) = match std::env::var("TERMLING_STOCKS") {
        Ok(v) if !v.trim().is_empty() => (v, true),
        _ => (DEFAULT_SYMBOLS.to_string(), false),
    };
    let symbols = raw
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if symbols.is_empty() {
        (vec![DEFAULT_SYMBOLS.to_string()], false)
    } else {
        (symbols, custom)
    }
}

/// Parse Stooq's `sd2t2ohlcv` CSV (with header). Rows whose price fields are
/// missing (`N/D`) or malformed are skipped. Change is close − open.
fn parse_csv(body: &str) -> Vec<Quote> {
    let mut out = Vec::new();
    for raw in body.lines().skip(1) {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        // Symbol, Date, Time, Open, High, Low, Close, Volume
        if f.len() < 7 {
            continue;
        }
        let symbol = f[0].trim().trim_end_matches(".US").trim_end_matches(".us");
        let (open, close) = match (f[3].trim().parse::<f64>(), f[6].trim().parse::<f64>()) {
            (Ok(o), Ok(c)) => (o, c),
            _ => continue,
        };
        let change = close - open;
        let change_pct = if open != 0.0 {
            change / open * 100.0
        } else {
            0.0
        };
        out.push(Quote {
            symbol: symbol.to_string(),
            close,
            change,
            change_pct,
        });
    }
    out
}

fn short(e: &str) -> String {
    let first = e.trim().lines().next().unwrap_or("").trim();
    first.chars().take(56).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    const SAMPLE: &str = "Symbol,Date,Time,Open,High,Low,Close,Volume\n\
        AAPL.US,2024-01-02,22:00:05,187.15,188.44,183.89,185.64,82488200\n\
        MSFT.US,2024-01-02,22:00:06,373.86,375.00,366.50,370.87,25258600\n";

    fn text(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parses_two_rows_with_signed_change() {
        let q = parse_csv(SAMPLE);
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].symbol, "AAPL");
        assert_eq!(q[0].close, 185.64);
        // 185.64 - 187.15 = -1.51 (down day)
        assert!(
            (q[0].change - (-1.51)).abs() < 1e-6,
            "change {}",
            q[0].change
        );
        assert!(q[0].change < 0.0);
        assert!(q[1].change < 0.0);
    }

    #[test]
    fn skips_missing_and_short_rows() {
        let body = "Symbol,Date,Time,Open,High,Low,Close,Volume\n\
            BAD.US,2024-01-02,22:00:05,N/D,N/D,N/D,N/D,N/D\n\
            short,row\n\
            GOOG.US,2024-01-02,22:00:05,140.00,142.00,139.00,141.50,1000\n";
        let q = parse_csv(body);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].symbol, "GOOG");
    }

    #[test]
    fn renders_prices_and_change() {
        let mut s = Stocks::new();
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok(SAMPLE.into()),
        });
        let out = text(&s.render(40, 12, &THEMES[0]));
        assert!(out.contains("AAPL"), "symbol: {out}");
        assert!(out.contains("185.64"), "price: {out}");
        assert!(out.contains("-1.51"), "change: {out}");
    }
}
