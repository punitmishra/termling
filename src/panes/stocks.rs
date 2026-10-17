//! A quotes pane backed by Yahoo Finance's key-less `v8/finance/chart` endpoint.
//!
//! We fetch one symbol per request (cycling through the watchlist, one per tick)
//! and read `regularMarketPrice` + `chartPreviousClose` from the response's
//! `meta`. Each response carries its own `symbol`, so results route back by the
//! ticker in the payload — no per-symbol request tagging needed. The watchlist
//! comes from `TERMLING_STOCKS="AAPL,MSFT"`, defaulting to a couple of tickers.
//! We show the last price and the day's change, colored up/down.
//!
//! (Stooq's CSV endpoint used to serve this but now 404s, hence Yahoo + the
//! default `User-Agent` the HTTP agent sends — Yahoo 429s requests without one.)

use super::{line, short, Pane, PaneKind};
use crate::doodle::{Row, Seg};
use crate::net::{self, FetchSpec};
use crate::theme::Theme;

/// Frames between refresh cycles (~5 min at the dashboard's ~10 ticks/second).
const REFRESH_FRAMES: u64 = 5 * 60 * 10;

/// Default watchlist when `TERMLING_STOCKS` is unset.
const DEFAULT_SYMBOLS: &str = "AAPL,MSFT";

/// One parsed quote.
struct Quote {
    symbol: String,
    price: f64,
    change: f64,
    change_pct: f64,
}

pub struct Stocks {
    symbols: Vec<String>,
    custom: bool,
    frame: u64,
    fetched_at: Option<u64>,
    /// Symbols still to request this cycle, stored reversed so `pop()` yields the
    /// watchlist's first symbol first (drained one per `poll_fetch`).
    queue: Vec<String>,
    /// True while a single request is in flight (one at a time).
    in_flight: bool,
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
            queue: Vec::new(),
            in_flight: false,
            quotes: Vec::new(),
            error: None,
        }
    }

    fn due(&self) -> bool {
        match self.fetched_at {
            None => true,
            Some(f) => self.frame.saturating_sub(f) >= REFRESH_FRAMES,
        }
    }

    /// Upsert a quote by symbol, so a refresh updates in place and the render
    /// order follows the configured watchlist.
    fn store(&mut self, q: Quote) {
        if let Some(slot) = self.quotes.iter_mut().find(|e| e.symbol == q.symbol) {
            *slot = q;
        } else {
            self.quotes.push(q);
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

    fn refresh(&mut self) {
        // Mark stale so the next poll starts a fresh cycle.
        self.fetched_at = None;
    }

    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        if !net::online_enabled() || self.in_flight {
            return None;
        }
        // Start a new cycle when the data is due and we're not mid-cycle. Build
        // the queue reversed so `pop()` requests the watchlist in listed order,
        // and quotes render in that order too (first-seen wins in `store`).
        if self.queue.is_empty() && self.due() {
            self.queue = self.symbols.iter().rev().cloned().collect();
            self.fetched_at = Some(self.frame);
        }
        let symbol = self.queue.pop()?;
        self.in_flight = true;
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
            symbol
        );
        Some(FetchSpec::get(PaneKind::Stocks, url))
    }

    fn on_data(&mut self, result: &crate::net::FetchResult) {
        self.in_flight = false;
        match &result.body {
            Err(e) => self.error = Some(short(e)),
            Ok(body) => match parse_quote(body) {
                Ok(q) => {
                    self.store(q);
                    self.error = None;
                }
                // A single bad symbol response shouldn't wipe the others.
                Err(e) if self.quotes.is_empty() => self.error = Some(e),
                Err(_) => {}
            },
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
                        text: format!("{:>9.2}  ", q.price),
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
        (vec!["AAPL".to_string(), "MSFT".to_string()], false)
    } else {
        (symbols, custom)
    }
}

/// Parse one Yahoo `v8/finance/chart` response into a [`Quote`] via its `meta`
/// block. Returns a short error string on an API error or missing fields.
fn parse_quote(body: &str) -> Result<Quote, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| short(&e.to_string()))?;
    let chart = &v["chart"];
    if !chart["error"].is_null() {
        return Err("api error".into());
    }
    let meta = &chart["result"][0]["meta"];
    let symbol = meta["symbol"].as_str().ok_or("no symbol")?.to_string();
    let price = meta["regularMarketPrice"].as_f64().ok_or("no price")?;
    // Fall back to previousClose if chartPreviousClose is absent.
    let prev = meta["chartPreviousClose"]
        .as_f64()
        .or_else(|| meta["previousClose"].as_f64())
        .unwrap_or(price);
    let change = price - prev;
    let change_pct = if prev != 0.0 {
        change / prev * 100.0
    } else {
        0.0
    };
    Ok(Quote {
        symbol,
        price,
        change,
        change_pct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;

    // A trimmed-but-real-shaped v8 chart response.
    const SAMPLE_AAPL: &str = r#"{"chart":{"result":[{"meta":{"currency":"USD",
        "symbol":"AAPL","regularMarketPrice":185.64,"chartPreviousClose":187.15}}],
        "error":null}}"#;
    const SAMPLE_MSFT: &str = r#"{"chart":{"result":[{"meta":{"currency":"USD",
        "symbol":"MSFT","regularMarketPrice":420.10,"chartPreviousClose":418.00}}],
        "error":null}}"#;

    fn text(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parses_price_and_signed_change() {
        let q = parse_quote(SAMPLE_AAPL).expect("parse");
        assert_eq!(q.symbol, "AAPL");
        assert_eq!(q.price, 185.64);
        // 185.64 - 187.15 = -1.51 (down day)
        assert!((q.change - (-1.51)).abs() < 1e-6, "change {}", q.change);
        assert!(q.change < 0.0);
    }

    #[test]
    fn api_error_and_garbage_are_errors_not_panics() {
        assert!(parse_quote(r#"{"chart":{"result":null,"error":{"code":"x"}}}"#).is_err());
        assert!(parse_quote("not json").is_err());
        assert!(parse_quote("{}").is_err());
    }

    #[test]
    fn renders_price_and_change() {
        let mut s = Stocks::new();
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok(SAMPLE_AAPL.into()),
        });
        let out = text(&s.render(40, 12, &THEMES[0]));
        assert!(out.contains("AAPL"), "symbol: {out}");
        assert!(out.contains("185.64"), "price: {out}");
        assert!(out.contains("-1.51"), "change: {out}");
    }

    #[test]
    fn quotes_render_in_watchlist_order() {
        // The watchlist order must survive the fetch cycle: with "AAPL,MSFT",
        // AAPL is requested first and renders above MSFT. (Regression: `pop()`
        // used to drain the queue from the end, inverting the configured order.)
        std::env::set_var("TERMLING_ONLINE", "1");
        std::env::set_var("TERMLING_STOCKS", "AAPL,MSFT");
        let mut s = Stocks::new();

        // First poll requests AAPL (the first listed symbol), not MSFT.
        let first = s.poll_fetch().expect("a first fetch");
        assert!(first.url.contains("/AAPL?"), "first request: {}", first.url);
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok(SAMPLE_AAPL.into()),
        });

        // Second poll requests MSFT.
        let second = s.poll_fetch().expect("a second fetch");
        assert!(
            second.url.contains("/MSFT?"),
            "second request: {}",
            second.url
        );
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok(SAMPLE_MSFT.into()),
        });

        let out = text(&s.render(40, 12, &THEMES[0]));
        let aapl = out.find("AAPL").expect("AAPL present");
        let msft = out.find("MSFT").expect("MSFT present");
        assert!(aapl < msft, "AAPL should render above MSFT:\n{out}");

        std::env::remove_var("TERMLING_ONLINE");
        std::env::remove_var("TERMLING_STOCKS");
    }

    #[test]
    fn one_bad_response_keeps_existing_quotes() {
        let mut s = Stocks::new();
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok(SAMPLE_AAPL.into()),
        });
        // A later garbage response must not wipe the good AAPL quote.
        s.on_data(&crate::net::FetchResult {
            pane: PaneKind::Stocks,
            body: Ok("garbage".into()),
        });
        assert_eq!(s.quotes.len(), 1);
        assert_eq!(s.quotes[0].symbol, "AAPL");
    }
}
