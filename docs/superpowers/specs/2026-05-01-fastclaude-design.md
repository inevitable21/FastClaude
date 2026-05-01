# FastClaude — Design Spec

**Date:** 2026-05-01
**Status:** Approved for implementation planning

## Summary

FastClaude is a cross-platform desktop application that launches and manages live Claude Code sessions running in external terminal windows. It is a dashboard, not an embedded terminal: each session runs as an independent process in the user's preferred terminal, and FastClaude observes, focuses, and kills these sessions on demand.

## Goals

- One-click launch of new Claude Code sessions into a chosen project folder, with optional model selection and starting prompt.
- A live dashboard of every session FastClaude has launched: project, elapsed time, model, status (running / idle / ended), and rolling token cost.
- A global hotkey to summon the launch dialog from anywhere on the OS.
- Auto-discovered "recent projects" sourced from `~/.claude/projects/`.
- Cross-platform: Windows, macOS, Linux.
- Polished, easy-to-modify UI built on widely-known web technologies.

## Non-goals (v1)

- Browsing or resuming historical Claude sessions.
- Embedded terminal — sessions run in external terminal programs only.
- Discovering Claude sessions started outside the app.
- Tabbed-terminal awareness / tmux / zellij integration.
- Multi-machine, cloud, or remote sessions.
- User accounts, auth, mobile, web hosting, plugin system.
- Auto-launch on system boot.
- Internationalization — English only.

## Tech stack

- **Shell:** [Tauri 2](https://tauri.app/) — Rust backend, web frontend, ~10 MB installer, native to all three target OSes.
- **Frontend:** React + Tailwind CSS + shadcn/ui components.
- **Backend:** Rust core, with `sysinfo` for cross-platform process inspection, `rusqlite` for state, `tauri-plugin-global-shortcut` for the hotkey.
- **Persistence:** SQLite database + JSON config file under `%APPDATA%/FastClaude/` (and the OS-equivalent paths on macOS / Linux).

## Architecture

```
┌─ FastClaude (Tauri app) ─────────────────────────────────────────┐
│                                                                   │
│  Frontend (React + shadcn-ui)        Backend (Rust core)          │
│  ─────────────────────────────       ─────────────────────────    │
│  • Dashboard                         • session_registry (SQLite)  │
│  • LaunchDialog                      • spawner                    │
│  • Settings                          • poller (2 s tick)          │
│  • UsageStrip                        • cost_reader                │
│                                      • recent_projects            │
│                                      • window_focus (per-OS)      │
│                                      • hotkey                     │
│                                      • config                     │
│                                                                   │
└────────────────────┬────────────────┬────────────────┬────────────┘
                     │ spawn          │ scan           │ read
                     ▼                ▼                ▼
           ┌───────────────┐  ┌──────────────┐  ┌────────────────────┐
           │ External      │  │ OS process   │  │ ~/.claude/         │
           │ terminal      │  │ table        │  │ projects/          │
           │ (Windows      │  │              │  │ (recent dirs +     │
           │  Terminal,    │  │              │  │  session JSONLs)   │
           │  iTerm, etc.) │  │              │  │                    │
           └───────────────┘  └──────────────┘  └────────────────────┘

Local state: %APPDATA%/FastClaude/state.db, config.json
```

### Boundaries

- The frontend never touches the file system, the process table, or SQLite directly. All access is through Tauri commands.
- Each backend module exposes a small public surface and hides its internals. `window_focus` is one trait with three OS-specific implementations.
- Sessions live as independent OS processes. FastClaude observes them; closing FastClaude does not kill them.

## Backend modules

| Module | Responsibility |
|---|---|
| `session_registry` | SQLite CRUD over the `sessions` table. Single source of truth for app-launched sessions. |
| `spawner` | Given `(project_dir, model, prompt?)`, opens a new terminal window running `claude`. Captures terminal PID and window handle. |
| `poller` | 2 s tick: verifies stored PIDs against the OS process table, links sessions to their JSONL files, refreshes cost. Marks idle / ended sessions. |
| `cost_reader` | Stream-reads a session's JSONL file from a stored offset, sums input / output / cache tokens, multiplies by per-model pricing. |
| `recent_projects` | Scans `~/.claude/projects/` (decoding the path-encoded folder names) and returns mtime-sorted entries. Pure read. |
| `window_focus` | Trait with `win.rs` / `mac.rs` / `linux.rs` implementations. Brings a stored window handle to the foreground. |
| `hotkey` | Wraps `tauri-plugin-global-shortcut`. Hotkey fires → emits `hotkey-fired` IPC event. |
| `config` | Loads / saves `config.json`: terminal program, default model, hotkey binding, idle threshold, pricing table. |

## Frontend components

| Component | Responsibility |
|---|---|
| `Dashboard` | List of running sessions with status dot, elapsed time, model badge, Focus / Kill buttons. |
| `LaunchDialog` | Folder picker (with recents pre-filled), model picker, optional starting prompt. |
| `Settings` | Terminal program, hotkey, default model, pricing overrides, debug log viewer. |
| `UsageStrip` | Today / this-week tokens and cost. Subscribes to `usage-updated` events. |

## IPC contract

**Tauri commands:**
- `list_sessions() -> Session[]`
- `launch_session({ project_dir, model, prompt? }) -> session_id`
- `kill_session(id)`
- `focus_session(id)`
- `recent_projects(limit) -> Project[]`
- `get_config()` / `set_config(...)`

**Events emitted to frontend:**
- `session-changed` — registry mutated; refresh dashboard
- `usage-updated` — cost figures changed; refresh usage strip
- `hotkey-fired` — open launch dialog and bring window to front

## Data model

### `sessions` table

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT (UUID) | Primary key |
| `project_dir` | TEXT | Absolute path |
| `model` | TEXT | e.g. `claude-opus-4-7` |
| `claude_pid` | INTEGER | The `claude` process |
| `terminal_pid` | INTEGER | The terminal window process |
| `terminal_window_handle` | TEXT (nullable) | OS-specific (HWND on Windows, AX uid on macOS, X11 wid on Linux) |
| `started_at` | INTEGER (epoch seconds) | |
| `ended_at` | INTEGER (nullable) | Set when poller detects death |
| `jsonl_path` | TEXT (nullable) | Resolved on first poll tick |
| `jsonl_offset` | INTEGER | Last byte offset read by `cost_reader` |
| `status` | TEXT | `running` / `idle` / `ended` |
| `last_activity_at` | INTEGER | From JSONL mtime; drives idle detection |
| `tokens_in` / `tokens_out` / `tokens_cache_read` / `tokens_cache_write` | INTEGER | Cumulative |
| `cost_usd` | REAL | Cumulative |

Rows are never deleted. Ended sessions stay in the table with `ended_at` set, enabling lifetime cost summaries and a "last 24 h" history view.

### `config.json`

```json
{
  "terminal_program": "auto",
  "default_model": "claude-opus-4-7",
  "hotkey": "Ctrl+Shift+C",
  "idle_threshold_seconds": 300,
  "pricing": {
    "claude-opus-4-7":  { "input": 15.0, "output": 75.0, "cache_read": 1.5,  "cache_write": 18.75 },
    "claude-sonnet-4-6":{ "input": 3.0,  "output": 15.0, "cache_read": 0.3,  "cache_write": 3.75  },
    "claude-haiku-4-5": { "input": 1.0,  "output": 5.0,  "cache_read": 0.1,  "cache_write": 1.25  }
  }
}
```

Pricing is per million tokens. Users can override the table if Anthropic changes prices.

### Recent projects

Not persisted. Derived live from scanning `~/.claude/projects/` and decoding the path-encoded folder names. If the user wipes Claude history, the recent list updates accordingly.

## Key flows

### Launch a session

1. User clicks **+ Launch** or fires the global hotkey.
2. `LaunchDialog` opens with recent projects pre-filled.
3. User picks project dir, model, optional starting prompt; submits.
4. Frontend calls `launch_session(...)`.
5. `spawner` invokes the configured terminal program with the working directory and `claude --model <model>` (and pipes the starting prompt to stdin if provided).
6. Spawner captures terminal PID and attempts to capture the window handle (Windows: `EnumWindows` matched by PID; macOS: AppleScript window id; Linux: `wmctrl -lp`).
7. Inserts a row into `sessions` with `status='running'`, returns `session_id`.
8. Emits `session-changed` → dashboard re-renders.

### Poller tick (every 2 s)

1. Load all rows where `ended_at IS NULL`.
2. For each row: check `claude_pid` against the OS process table. If dead → set `ended_at = now`, `status='ended'`.
3. For still-running rows without `jsonl_path`: scan that project's `~/.claude/projects/<encoded>/` directory for the newest file with mtime ≥ `started_at`. Set `jsonl_path`.
4. For rows with `jsonl_path`: stat the file. If mtime advanced → call `cost_reader.update(row)`; bump `last_activity_at`. If mtime is older than `idle_threshold_seconds` → `status='idle'`.
5. Emit `session-changed` if anything changed; emit `usage-updated` if any cost changed.

### Focus a session

1. Frontend calls `focus_session(id)`.
2. `window_focus` impl tries `terminal_window_handle` first, falls back to `terminal_pid`, brings the window to the foreground.
3. If both fail → emit a non-fatal toast: "Couldn't find terminal window — opening project folder instead." Open the folder in the OS file manager.

### Kill a session

1. Show confirm dialog (avoid accidental termination of in-flight work).
2. Send `SIGTERM` to `claude_pid`. After 2 s, if still alive → `SIGKILL`.
3. Poller detects the death on the next tick.

### App startup

1. Load all rows with `status` in (`running`, `idle`).
2. For each: verify `claude_pid` is still alive.
   - Alive → keep as is. Stored window handle may be stale; refresh on next focus attempt.
   - Dead → set `ended_at = now`, `status='ended'`.
3. Start the poller. Render dashboard.

### Global hotkey

1. OS fires the registered shortcut.
2. `hotkey` module emits `hotkey-fired`.
3. Frontend opens `LaunchDialog` and brings the FastClaude window to the foreground.

### Cost computation (in `cost_reader`)

1. Open the JSONL file at the stored byte offset.
2. For each `assistant` event read: sum `usage.input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`.
3. Multiply by per-model pricing from config; add to the row's totals.
4. Persist the new offset.

## Per-OS specifics

### Terminal spawn

| OS | Default | Command pattern |
|---|---|---|
| Windows | Windows Terminal (`wt.exe`); fallback `cmd.exe` | `wt.exe -d "<dir>" claude --model <m>` |
| macOS | Terminal.app; iTerm if installed | AppleScript: `tell app "Terminal" to do script "cd <dir> && claude --model <m>"` |
| Linux | `gnome-terminal` → `konsole` → `xterm` | `gnome-terminal --working-directory=<dir> -- claude --model <m>` |

Configurable via Settings. Power users can supply their own argv template with `{dir}` and `{cmd}` placeholders.

### Window focus

| OS | Mechanism |
|---|---|
| Windows | `EnumWindows` → match `GetWindowThreadProcessId` against `terminal_pid` → store HWND → `SetForegroundWindow(HWND)` (with `AllowSetForegroundWindow` workaround). |
| macOS | AppleScript: `tell app "Terminal" to set frontmost of window id <wid> to true`. iTerm has an equivalent. Window id captured at spawn. |
| Linux (X11) | `wmctrl -lp` to map PID → window ID, `wmctrl -ia <id>` to focus. Falls back to `xdotool`. |

### Process detection

All three OSes: `sysinfo` crate. No per-OS code for "is PID alive" or process scanning.

### Global hotkey

`tauri-plugin-global-shortcut` covers all three OSes. macOS may prompt for Accessibility permissions on first launch — onboarding surfaces this.

### Documented limitations

- **Wayland (Linux)**: window focus is unreliable by design. We detect Wayland and degrade Focus to "open project folder."
- **Tabbed terminals**: when the user's terminal opens new sessions as tabs in an existing window (default Windows Terminal behavior), Focus brings the *window* forward — the user finds the right tab themselves. v1 accepts this.
- **Terminal multiplexers** (tmux, zellij): out of scope. Power users can launch into them via the argv template, but Focus only knows the outer terminal window.

## Testing

### Rust backend (unit)

- `cost_reader`: feed fixture JSONL, assert tallies. Fixtures pinned against real Claude session samples in `tests/fixtures/`.
- `recent_projects`: fixture directory tree → assert decoded paths and ordering.
- `session_registry`: in-memory SQLite, CRUD round-trips.
- `spawner` and `window_focus`: behind traits; mock OS calls. Real-OS integration tests gated by `#[cfg(test)]` and an env flag, run manually.
- `poller`: inject a fake clock and a fake registry, assert state transitions.

### Frontend (component)

- `LaunchDialog`: folder picker behavior, model selection, prompt submission.
- `Dashboard` row rendering for each status.
- Tauri command layer mocked.

### End-to-end

- One smoke test per OS that actually spawns a real `claude` and tears it down. Manual checklist for v1, automation later.

## Error handling principles

- **Fail loud in the dashboard, not silently.** Failed Tauri commands surface a toast; the error gets a single-line entry in a debug log panel under Settings → Logs.
- **Never crash the app on a session failure.** A failed spawn leaves no row; a poll error for one session does not stop the poller.
- **Window focus failure is non-fatal** — toast plus "open project folder" fallback.
- **Pricing missing for a model** — show tokens, show `cost: —` with a note. Don't guess.
- **Stale state on startup** — be paranoid: every stored PID is verified before being trusted.

## Open questions deferred to implementation

- Exact crate choices for `rusqlite` migrations (refinery vs raw).
- React routing approach (none vs minimal — single-window app may not need a router).
- Tauri plugin choice for opening the OS file manager from `Focus` fallback.
- Packaging / signing strategy per OS.
- **Windows Terminal PID resolution.** `wt.exe` may exit immediately after handing off the new tab to a running terminal host; the launched process is not the long-lived terminal. The implementation should resolve `terminal_pid` as the parent process of `claude_pid` after spawn (with a brief retry to account for the spawn race). This is true on macOS / Linux too and may simplify the cross-platform code.

These are implementation details, not design questions, and will be resolved when the implementation plan is written.
