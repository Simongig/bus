# bus

Minimal timeblocking for the terminal. Plan your day as **stops** on a route. When the day slips,
push stops around: later stops move along, and free time absorbs the push.

```
 bus  Wed 04 Nov 2026 (today)   2 stops · 1h45m planned
┌ Route ──────────────────────────────┐┌ Stop ───────────────────────────────┐
│09:00   Deep work · 1h30m  [1/2]     ││Deep work                            │
│     ▶                               ││09:00–10:30 · 1h30m                  │
│                                     ││                                     │
│10:00                                ││Definition of done                   │
│                                     ││1 [x] draft                          │
│                                     ││2 [ ] review                         │
│10:30   Break · 15m                  ││                                     │
│        free 1h45m                   ││                                     │
└─────────────────────────────────────┘└─────────────────────────────────────┘
 a add · e edit · b break · B lunch · J/K move · +/- length · h/l day · ? help · q quit
```

## Install

Download the latest `bus-*-x86_64-linux.tar.gz` from Releases, then:

```sh
tar -xzf bus-*-x86_64-linux.tar.gz -C ~/.local/bin
```

Or build from source (needs Rust and a C compiler, e.g. `sudo apt install build-essential`):

```sh
cargo install --git https://github.com/Simongig/bus
```

## Use

```sh
bus        # open today's plan; leave it open in a tab to get notifications
bus now    # one line: "Deep work 42m → Break 10:30"
```

| Key | Action |
| --- | --- |
| `j` / `k` | select next / previous stop |
| `J` / `K` | move stop 15m later / earlier |
| `+` / `-` | make stop 15m longer / shorter |
| `a` | add a stop |
| `e`, `Enter` | edit the selected stop |
| `d` | delete the selected stop |
| `b` / `B` | insert a 15m break / 60m lunch after the selection |
| `Space` | check the next open definition-of-done item |
| `1`–`9` | toggle a definition-of-done item by number |
| `h` / `l` | previous / next day |
| `t` | jump to today |
| `?` | help |
| `q` | quit |

Times snap to 15 minutes. Durations accept `45m`, `90`, `1h30` or `1.5h`.

**Ripple rules:** moving a stop later or making it longer pushes the stops it now overlaps. A free gap
absorbs the push. Moving earlier or shrinking never pulls other stops. A push past midnight is refused.

While `bus` is running, you get a desktop notification when each stop starts and ends. The end
notification lists any definition-of-done items that are still open.

### Status bar

`bus now --format '{title} {left}'` supports `{title}`, `{left}`, `{next}`, `{next_start}` and `{next_in}`.

- tmux: `set -g status-right '#(bus now)'` and `set -g status-interval 30`
- GNOME top bar: use an extension that shows command output (e.g. *Executor*) with the command `bus now`.

## Data

Plans are stored in SQLite at `~/.local/share/bus/bus.db`. To use another file, set `BUS_DB=/path/to/file.db`.

## Development

`.devcontainer/Dockerfile` is a sandbox for running Claude Code on this repo:

```sh
docker build -t bus-claude -f .devcontainer/Dockerfile .
docker run -it --rm -v "$PWD":/workspace bus-claude
```
