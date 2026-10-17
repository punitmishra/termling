//! The network boundary for termling's opt-in online panes.
//!
//! termling is zero-network by default. This module is the *one* place a network
//! call can happen, and only when the crate is built `--features online` **and**
//! the user sets `TERMLING_ONLINE=1` (see [`online_enabled`]). It mirrors the
//! fire-and-forget, non-blocking ethos of the shell socket (`crate::event`): a
//! single background worker thread does the blocking HTTP, and results come back
//! over an `mpsc` channel that the render loop drains without ever blocking.
//!
//! The boundary is deliberately generic — it moves bytes, it does not know about
//! weather or stocks. A [`Pane`](crate::panes::Pane) asks for a [`FetchSpec`]
//! (method + url + headers + optional body), tagged with its own
//! [`PaneKind`](crate::panes::PaneKind); the worker returns a [`FetchResult`]
//! tagged the same way, and the pane parses its own payload in
//! [`Pane::on_data`](crate::panes::Pane::on_data). That keeps every API-specific
//! detail inside the pane that owns it.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::panes::PaneKind;

/// HTTP method for a fetch. Only the two the panes need.
// Constructed by the online panes (weather/stocks/news land in WS-C, AI's POST in
// WS-D); the allow falls away once those consumers exist.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A request a pane wants performed off-thread.
// Fields (`body`/`headers`) and the `get` constructor are exercised by the online
// panes (WS-C/WS-D); allow until those land.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FetchSpec {
    /// Which pane asked — used to route the result back to it.
    pub pane: PaneKind,
    pub method: Method,
    pub url: String,
    /// Extra request headers (e.g. an API key for the AI pane).
    pub headers: Vec<(String, String)>,
    /// Request body for `Post`.
    pub body: Option<String>,
}

#[allow(dead_code)] // `get` is the constructor the online data panes (WS-C) use.
impl FetchSpec {
    /// A plain GET for `pane`.
    pub fn get(pane: PaneKind, url: impl Into<String>) -> FetchSpec {
        FetchSpec {
            pane,
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }
}

/// The outcome of a [`FetchSpec`], tagged with the pane that asked. `body` is the
/// response text on success, or a short error string the pane can surface.
// `body` is read by the online panes' `on_data` (WS-C/WS-D); allow until they land.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub pane: PaneKind,
    pub body: Result<String, String>,
}

/// A handle to the background fetch worker: send requests, drain results. Dropping
/// it closes the request channel, which ends the worker thread.
pub struct Net {
    req_tx: Sender<FetchSpec>,
    res_rx: Receiver<FetchResult>,
}

impl Net {
    /// Spawn the worker thread and return a handle. Created only in online mode, so
    /// the default build never starts a thread.
    pub fn spawn() -> Net {
        let (req_tx, req_rx) = mpsc::channel::<FetchSpec>();
        let (res_tx, res_rx) = mpsc::channel::<FetchResult>();
        std::thread::Builder::new()
            .name("termling-net".into())
            .spawn(move || {
                // Blocks only this worker; the UI thread is never affected.
                while let Ok(spec) = req_rx.recv() {
                    let pane = spec.pane;
                    let body = fetch_blocking(&spec);
                    if res_tx.send(FetchResult { pane, body }).is_err() {
                        break; // UI gone — stop.
                    }
                }
            })
            .expect("spawn net worker");
        Net { req_tx, res_rx }
    }

    /// Queue a request. Never blocks; a dead worker simply drops it.
    pub fn request(&self, spec: FetchSpec) {
        let _ = self.req_tx.send(spec);
    }

    /// Every completed result waiting right now (non-blocking).
    pub fn try_drain(&self) -> Vec<FetchResult> {
        self.res_rx.try_iter().collect()
    }
}

/// Whether networked panes should actually run: the crate must be built with the
/// `online` feature **and** the user must opt in at runtime with `TERMLING_ONLINE=1`.
/// Both gates default off, so a stock build makes no network calls.
pub fn online_enabled() -> bool {
    cfg!(feature = "online") && matches!(std::env::var("TERMLING_ONLINE").as_deref(), Ok("1"))
}

/// Perform the request, blocking. Real HTTP only under `--features online`;
/// otherwise a clear error so a pane can say "built without online".
#[cfg(feature = "online")]
fn fetch_blocking(spec: &FetchSpec) -> Result<String, String> {
    use std::sync::OnceLock;
    use std::time::Duration;
    // One agent, built once and reused across every fetch. Without explicit
    // timeouts a single hung host would block this worker thread forever and
    // wedge every online pane, so cap both the connect and the read at ~8s.
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout_read(Duration::from_secs(8))
            // A real User-Agent: some free APIs (e.g. Yahoo's quote endpoint)
            // 429 or reject requests without one.
            .user_agent(concat!("termling/", env!("CARGO_PKG_VERSION")))
            .build()
    });
    let mut req = match spec.method {
        Method::Get => agent.get(&spec.url),
        Method::Post => agent.post(&spec.url),
    };
    for (k, v) in &spec.headers {
        req = req.set(k, v);
    }
    let resp = match spec.method {
        Method::Get => req.call(),
        Method::Post => req.send_string(spec.body.as_deref().unwrap_or("")),
    };
    match resp {
        Ok(r) => r.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(feature = "online"))]
fn fetch_blocking(_spec: &FetchSpec) -> Result<String, String> {
    Err("termling was built without the `online` feature".into())
}
