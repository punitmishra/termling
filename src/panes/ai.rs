//! A Claude-backed dashboard pane: one short, dev-oriented tip per session.
//!
//! This is the only pane that talks to a paid LLM, so it is deliberately frugal.
//! It reads `ANTHROPIC_API_KEY` from the environment *at fetch time* (never stored,
//! never logged — it travels only in the request headers), fetches exactly once
//! per session through [`crate::net`], and otherwise sits quiet. If the key is
//! missing it renders a hint and never issues a request.
//!
//! The request is a single [Messages API] call to a small, fast, inexpensive model
//! (`claude-haiku-5-5`) asking for one concise tip. The request *shape* and the
//! response *parser* are pure, unit-tested helpers ([`request_headers`],
//! [`request_body`], [`parse_tip`]) so the HTTP seam is covered without ever
//! touching the network in tests.
//!
//! [Messages API]: https://docs.anthropic.com/en/api/messages

use crate::doodle::Row;
use crate::net::{self, FetchResult, FetchSpec, Method};
use crate::panes::{line, Pane, PaneKind};
use crate::theme::Theme;

/// The environment variable that holds the Claude API key.
const ENV_KEY: &str = "ANTHROPIC_API_KEY";

/// The Messages API endpoint.
const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// A fast, inexpensive current model — right-sized for one short tip.
const MODEL: &str = "claude-haiku-5-5";

/// The pinned Messages API version header.
const API_VERSION: &str = "2023-06-01";

/// A small ceiling: a one-liner tip needs very little, and this caps spend.
const MAX_TOKENS: u32 = 256;

/// The prompt. Kept terse on purpose — we want one tip, not an essay.
const PROMPT: &str = "Give exactly one concise, practical terminal or developer-productivity tip. \
     One or two sentences, no preamble, no markdown, no numbering.";

/// Where the pane is in its single fetch lifecycle.
enum Load {
    /// No API key in the environment — render the hint, never fetch.
    NeedKey,
    /// A key is present and we have not fetched yet this session.
    Pending,
    /// A request is in flight.
    Loading,
    /// A tip arrived.
    Ready(String),
    /// The fetch or parse failed; carries a short, readable reason.
    Failed(String),
}

/// The Claude tip pane.
pub struct Ai {
    state: Load,
}

impl Ai {
    /// A fresh pane, armed to fetch once a key is seen.
    pub fn new() -> Ai {
        Ai {
            state: Load::Pending,
        }
    }
}

impl Default for Ai {
    fn default() -> Self {
        Ai::new()
    }
}

impl Pane for Ai {
    fn title(&self) -> String {
        "ai tip".into()
    }

    fn render(&self, width: u16, _height: u16, theme: &Theme) -> Vec<Row> {
        match &self.state {
            Load::NeedKey => vec![
                line("no API key.", theme.bad),
                line("", theme.dim),
                line("set ANTHROPIC_API_KEY", theme.dim),
                line("to enable claude tips.", theme.dim),
            ],
            Load::Pending | Load::Loading => vec![line("fetching a tip…", theme.dim)],
            Load::Ready(tip) => {
                let mut rows = vec![line("claude says:", theme.dim), line("", theme.dim)];
                rows.extend(wrap(tip, width).into_iter().map(|l| line(l, theme.accent)));
                rows
            }
            Load::Failed(err) => {
                let mut rows = vec![
                    line("couldn't fetch a tip:", theme.dim),
                    line("", theme.dim),
                ];
                rows.extend(wrap(err, width).into_iter().map(|l| line(l, theme.bad)));
                rows
            }
        }
    }

    fn poll_fetch(&mut self) -> Option<FetchSpec> {
        // Defense-in-depth: this is the pane that spends money and carries the
        // key, so double-gate on online mode exactly like the other online panes
        // before we even look at the key.
        if !net::online_enabled() {
            return None;
        }
        // Read the key fresh, at fetch time — it is never stored on the pane.
        match current_key() {
            None => {
                // No key: make sure we show the hint and never fetch.
                self.state = Load::NeedKey;
                None
            }
            Some(key) => {
                // A key appeared after we were blocked — re-arm for one fetch.
                if matches!(self.state, Load::NeedKey) {
                    self.state = Load::Pending;
                }
                // Fetch at most once per session: only while still pending. The
                // dashboard pumps this every tick and again on `r`, so anything
                // other than `Pending` must stay quiet to avoid repeat spend.
                if matches!(self.state, Load::Pending) {
                    self.state = Load::Loading;
                    Some(FetchSpec {
                        pane: PaneKind::Ai,
                        method: Method::Post,
                        url: ENDPOINT.to_string(),
                        headers: request_headers(&key),
                        body: Some(request_body()),
                    })
                } else {
                    None
                }
            }
        }
    }

    fn refresh(&mut self) {
        // Re-arm the once-per-session one-shot so `r` fetches a fresh tip —
        // unless a request is already in flight (avoid a double spend).
        if !matches!(self.state, Load::Loading) {
            self.state = Load::Pending;
        }
    }

    fn on_data(&mut self, result: &FetchResult) {
        self.state = match &result.body {
            Ok(body) => match parse_tip(body) {
                Ok(tip) => Load::Ready(tip),
                Err(e) => Load::Failed(e),
            },
            Err(e) => Load::Failed(short_error(e)),
        };
    }
}

/// The API key, read from the environment, treated as absent if blank.
fn current_key() -> Option<String> {
    std::env::var(ENV_KEY).ok().filter(|k| !k.trim().is_empty())
}

/// The headers for a Messages API call. The key goes *only* here, never logged.
fn request_headers(key: &str) -> Vec<(String, String)> {
    vec![
        ("x-api-key".to_string(), key.to_string()),
        ("anthropic-version".to_string(), API_VERSION.to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ]
}

/// The JSON request body: a single-user-message Messages API call. Deterministic
/// (no timestamps / ids) so it is trivially unit-testable and cache-friendly.
fn request_body() -> String {
    serde_json::json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "messages": [{ "role": "user", "content": PROMPT }],
    })
    .to_string()
}

/// Pull the assistant's tip out of a Messages API response, or a short readable
/// error. Handles both a success payload (`content[].text`) and an API error
/// payload (`error.message`).
fn parse_tip(body: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "bad response from the API".to_string())?;

    // An API error comes back as {"type":"error","error":{"message": ...}}.
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown API error");
        return Err(msg.to_string());
    }

    // Success: the first text block in `content`.
    let text = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| {
            blocks.iter().find_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
        });

    match text {
        Some(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        _ => Err("the API returned no tip".to_string()),
    }
}

/// Keep transport error strings short and one-line for the tile.
fn short_error(e: &str) -> String {
    e.lines()
        .next()
        .unwrap_or("network error")
        .trim()
        .to_string()
}

/// Greedy word-wrap to `width` columns. A word longer than the line is left whole
/// for the UI to clip. Always returns at least one (possibly empty) line.
fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = (width.max(1)) as usize;
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
        } else if cur.chars().count() + 1 + word.chars().count() <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_has_the_expected_messages_api_shape() {
        let body = request_body();
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON body");

        assert_eq!(v["model"], "claude-haiku-5-5");
        assert_eq!(v["max_tokens"], 256);
        let msgs = v["messages"].as_array().expect("messages array");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let content = msgs[0]["content"].as_str().expect("string content");
        assert!(
            content.to_lowercase().contains("tip"),
            "prompt should ask for a tip: {content}"
        );
        // Deterministic body: no stray fields that would break cache / tests.
        assert!(v.get("tools").is_none());
    }

    #[test]
    fn headers_carry_the_key_and_the_required_metadata() {
        let headers = request_headers("sk-ant-secret");
        let get = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("x-api-key"), Some("sk-ant-secret"));
        assert_eq!(get("anthropic-version"), Some("2023-06-01"));
        assert_eq!(get("content-type"), Some("application/json"));
        // The key must live ONLY in the x-api-key header — nowhere else.
        let elsewhere = headers
            .iter()
            .filter(|(k, _)| k != "x-api-key")
            .any(|(_, v)| v.contains("sk-ant-secret"));
        assert!(!elsewhere, "the key must not leak into other headers");
        // And it is never baked into the request body.
        assert!(!request_body().contains("sk-ant-secret"));
    }

    /// A trimmed-down but realistic Messages API success payload.
    const SAMPLE_OK: &str = r#"{
        "id": "msg_01ABC",
        "type": "message",
        "role": "assistant",
        "model": "claude-haiku-5-5",
        "content": [
            { "type": "text", "text": "Use `cd -` to jump back to your previous directory." }
        ],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": 24, "output_tokens": 17 }
    }"#;

    /// An API error payload (HTTP-200-with-error or forwarded error body).
    const SAMPLE_ERR: &str = r#"{
        "type": "error",
        "error": { "type": "authentication_error", "message": "invalid x-api-key" }
    }"#;

    #[test]
    fn parser_extracts_the_tip_from_a_real_response() {
        let tip = parse_tip(SAMPLE_OK).expect("a tip");
        assert_eq!(tip, "Use `cd -` to jump back to your previous directory.");
    }

    #[test]
    fn parser_surfaces_api_errors_readably() {
        let err = parse_tip(SAMPLE_ERR).unwrap_err();
        assert_eq!(err, "invalid x-api-key");
    }

    #[test]
    fn parser_handles_a_thinking_block_before_the_text() {
        // Some models prepend a (possibly empty) thinking block; take the text.
        let body = r#"{
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "Pipe to `| less -R` to keep color." }
            ]
        }"#;
        assert_eq!(
            parse_tip(body).unwrap(),
            "Pipe to `| less -R` to keep color."
        );
    }

    #[test]
    fn parser_rejects_empty_and_garbage_payloads() {
        assert!(parse_tip("not json at all").is_err());
        assert!(parse_tip(r#"{"content": []}"#).is_err());
        assert!(parse_tip(r#"{"content": [{"type":"text","text":"   "}]}"#).is_err());
    }

    #[test]
    fn wrap_breaks_on_width_and_never_empties() {
        let lines = wrap("one two three four five", 9);
        assert!(lines
            .iter()
            .all(|l| l.chars().count() <= 9 || !l.contains(' ')));
        assert!(!lines.is_empty());
        assert_eq!(wrap("", 10), vec![String::new()]);
    }

    #[test]
    fn on_data_moves_through_the_states() {
        use crate::theme::THEMES;
        let theme = &THEMES[0];

        let mut pane = Ai::new();
        pane.on_data(&FetchResult {
            pane: PaneKind::Ai,
            body: Ok(SAMPLE_OK.to_string()),
        });
        assert!(matches!(pane.state, Load::Ready(_)));
        assert!(!pane.render(40, 12, theme).is_empty());

        pane.on_data(&FetchResult {
            pane: PaneKind::Ai,
            body: Err("dns error: no route".to_string()),
        });
        assert!(matches!(pane.state, Load::Failed(_)));
        assert!(!pane.render(40, 12, theme).is_empty());
    }
}
