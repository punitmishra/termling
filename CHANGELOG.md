# Changelog

All notable changes to termling. This project adheres to [semantic versioning](https://semver.org/)
(pre-1.0: minor features and fixes land in patch releases).

## 0.1.1

### Added
- **Clock pane** — a local, zero-network dashboard pane showing the date, time,
  timezone, and a current-month calendar with today highlighted.
- **Dashboard persistence** — the selected pane, layout, and buddy-strip state
  are remembered across restarts.
- **`b` key** toggles the buddy strip for a full-height dashboard.
- **Pane options** — `TERMLING_WEATHER_UNITS` (°C/°F) and `TERMLING_NEWS_COUNT`
  (1–12 headlines) env knobs.
- **Prebuilt online binaries** — each release now ships both a default
  (zero-network) and an `-online` (`--features online`) tarball per platform;
  `TERMLING_INSTALL_ONLINE=1` in the installer fetches the online one.

### Changed
- **Faster shell hooks** — the per-command `emit` is now backgrounded/disowned
  (zsh/bash/fish), so it never blocks the prompt (~5ms → sub-ms perceived).
- Online panes only fetch while the dashboard is open; the networked HTTP agent
  sends a `User-Agent` and has connect/read timeouts.
- Stocks pane moved from Stooq (endpoint removed) to Yahoo Finance.

### Internal
- Factored the online panes' refresh cadence into a shared `RefreshTimer`.

## 0.1.0

Initial release.

### Added
- A mood-driven ASCII **creature** that reacts to your real shell over a
  fire-and-forget Unix datagram socket (zsh/bash/fish hooks); **XP, streaks,
  levels, themes**, and an **arcade** of mini-games (with difficulty/speed options).
- A **buddy brain** — a local, non-LLM learner that watches your commands and
  talks back in a speech bubble.
- A sidebar **dashboard** (`d`): local panes (git, system, buddy insights) plus
  opt-in **online** panes (weather, stocks, news, dev-news, and a Claude ai tip)
  behind the `online` feature + `TERMLING_ONLINE=1` — both off by default.
- **Zero network calls** and only two dependencies in the default build.
- Packaging: GitHub Release tarballs + a `curl` installer; a wired-but-token-gated
  crates.io / Homebrew release pipeline.
