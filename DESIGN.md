# bus — terminal timeblocking app (design, MVP)

Status: design agreed via Q&A on 2026-11-04; implementation started.

## Concept
A minimal timeblocking tool. Plan a day as a sequence of time blocks ("stops" on the day's
"route") and adjust them during the day when things slip. Single user, local, Linux.

## Scope decisions
- **Platform:** Linux (Ubuntu) only for now. Desktop/laptop only, no mobile.
- **Interface:** terminal UI only. A local web UI (`bus serve`) may come later on the same core and DB.
- **Storage:** local SQLite file (e.g. `~/.local/share/bus/bus.db`). No sync. Work and private are
  separate because they live on separate machines.
- **Future online:** all DB access goes through a `Store` trait (`SqliteStore` now). Later backends could be
  libSQL/sqld (self-hosted) or a small HTTP API on the user's server. No auth or async in the MVP.
- **Standalone:** no calendar integration.
- **Audience:** only the author for now. Others may clone it or it may become a product later.

## Domain model
- **Day:** date + ordered list of stops. All days are kept (history).
- **Stop (block):** title, start time, duration (entered as duration, e.g. `90m`), optional
  definition-of-done list of checkable items `{text, done}`.
- No tags, categories, colours, priorities, or done/skipped status for the stop itself.
- 15-minute grid: all starts and durations snap to 15 min.
- Stops must stay within 00:00–24:00 of their day.

## Re-blocking (ripple)
- Moving a stop later or growing it pushes only the stops it would overlap, cascading. Free gaps
  absorb the shift and stop the ripple.
- Moving earlier or shrinking does not pull other stops back.
- If the ripple would push any stop past midnight, the action fails with a short message.

## TUI (ratatui)
- Day view: a vertical timeline of 06:00–23:00 that scrolls to keep "now" visible, with a now-line and
  gaps shown as free time.
- Navigate to the previous or next day (plan tomorrow, view past days). No week view.
- Keys (vim-style), with a `?` help overlay:
  - `j/k` select stop · `J/K` move ±15 min (ripple) · `+/-` duration ±15 min
  - `a` add · `e` edit · `d` delete · `b` insert 15-min Break / `B` insert 60-min Lunch after the selected stop (ripple)
  - `h/l` previous/next day · `Space` toggle DoD item · `q` quit
- No recurring blocks, templates, or copy-day.

## Notifications
- Sent only while the TUI is running (the user keeps a terminal tab open). A background daemon comes later.
- At start: "▶ <title>, <duration>". At end: "■ <title> done → next: <next>", plus any unchecked DoD items.

## `bus now` (peek)
- One-shot command that prints plain text on one line, e.g. `Deep work 42m → Lunch 12:30`.
- Intended for tmux or a GNOME top-bar extension (e.g. Executor). There is a `--format` template flag.

## Tech stack
- Rust: `ratatui` + `crossterm`, `rusqlite` (feature `bundled`), `notify-rust`, `chrono`, `clap`.
- Release: a GitHub Action builds `x86_64-unknown-linux-musl` on a `v*` tag and attaches it to the GitHub Release.
  Install into `~/.local/bin` or via `cargo install --git`.

## Day rollover
- If the TUI is open past midnight while showing today, it switches to the new day by itself.
