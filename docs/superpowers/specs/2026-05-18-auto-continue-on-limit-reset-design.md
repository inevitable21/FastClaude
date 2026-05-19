# Auto-continue on 5-hour limit reset — Design Spec

**Date:** 2026-05-18
**Status:** Approved for implementation planning

## Summary

When a Claude Code session hits its 5-hour usage window, FastClaude detects the rate-limit event from the session's JSONL, waits until the reset time, and respawns the session with `claude --resume <id>` plus a "continue" prompt — so work that hit the wall while the user was AFK picks up automatically. The feature is per-session opt-in, capped at N auto-resumes to prevent runaway loops, and survives FastClaude restarts.

## Goals

- Detect a rate-limit signal in a session's JSONL automatically.
- Wait until the reset timestamp and respawn via the existing spawner with `--resume` + a configurable continue prompt.
- Per-session opt-in: nothing acts on a session unless the user armed it.
- Bounded retries: a default cap of 3 auto-resumes per session chain prevents runaway loops.
- Restart-safe: a `next_resume_at` arriving while FastClaude was closed fires on next startup.

## Non-goals

- Injecting keystrokes into a foreign terminal window. (Auto-resume always spawns a fresh terminal via the existing `Spawner`.)
- Auto-arming all sessions. Opt-in only; default is off.
- Resuming sessions FastClaude didn't launch.
- A scheduling UI beyond a checkbox and a toggle. The user does not pick the resume time — it comes from the JSONL signal (with a fallback heuristic).

## Architecture

No new modules. Three existing pieces gain responsibilities:

- **`poller`** — gains rate-limit detection (during the JSONL read it already does) and a "fire pending resumes" step at the end of each tick.
- **`session_registry`** — gains six columns and small CRUD methods.
- **`usage_reader`** — gains a second return field `limit_event: Option<LimitEvent>` so a rate-limit signal is reported in the same pass that tallies tokens. No second JSONL read.

The spawner does not change — `SpawnRequest.resume` already exists. The Settings UI gains one section; the Dashboard session row gains a toggle; the LaunchDialog gains a checkbox.

```
┌─ poller tick ─────────────────────────────────────────────┐
│  for each active session:                                 │
│    alive check ─ existing                                 │
│    locate jsonl ─ existing                                │
│    usage_reader.read_delta() ─ now also returns           │
│       Option<LimitEvent { reset_at, detected_at }>        │
│    if limit_event && session.auto_continue:               │
│       registry.set_pending_resume(id, reset_at)           │
│  for each row where next_resume_at <= now                 │
│                       AND resume_count < resume_cap       │
│                       AND auto_continue:                  │
│    fire_resume(session) ─ new                             │
└───────────────────────────────────────────────────────────┘
```

## Data model

### `sessions` table — new columns

| Column | Type | Default | Notes |
|---|---|---|---|
| `auto_continue` | INTEGER (bool) | 0 | User opt-in. Settable from LaunchDialog at start or Dashboard mid-session. |
| `resume_prompt` | TEXT (nullable) | NULL | Per-session override. `NULL` → fall back to `config.default_resume_prompt` at fire time (not at launch time). |
| `next_resume_at` | INTEGER (nullable, epoch s) | NULL | Set when poller detects a rate-limit. Cleared after firing or when user toggles off. |
| `resume_count` | INTEGER | 0 | Increments on each auto-resume attempt. On a resumed row, initialized to predecessor's `resume_count + 1` so the cap counts the entire chain. |
| `resume_cap` | INTEGER | (from config at launch) | Per-session ceiling, frozen at launch time so a later change to the global default does not retroactively re-arm finished caps. |
| `resumed_into` | TEXT (nullable) | NULL | Pointer to the new session id created by an auto-resume — for UI threading and chain accounting. |
| `resume_failures` | INTEGER | 0 | Consecutive spawn failures at fire time. Resets to 0 on a successful spawn. |

All columns are `ALTER TABLE ADD COLUMN`-friendly with defaults, so the migration is in-place at `session_registry::open` startup.

### `config.json` — new fields

```jsonc
{
  "default_resume_prompt": "continue",
  "default_auto_continue": false,
  "default_resume_cap": 3
}
```

All three have safe `serde(default)` values matching the table above, so existing config files upgrade transparently.

### `LimitEvent` type (in `usage_reader`)

```rust
pub struct LimitEvent {
    pub reset_at: i64,        // epoch seconds, parsed from claude's message
    pub detected_at: i64,     // epoch seconds, when usage_reader read the line
}
```

Embedded in `UsageDelta`:

```rust
pub struct UsageDelta {
    // ...existing fields...
    pub limit_event: Option<LimitEvent>,
}
```

## Detection

`usage_reader::read_delta` already walks new bytes line-by-line. Extend the line loop to recognize a rate-limit signal. The exact JSONL shape Claude writes on rate-limit will be verified against a real sample during implementation (see Open questions). Two likely shapes — the reader tries both:

1. An `assistant` line whose `message.content` includes wording like `"5-hour limit"`, `"limit reached"`, or `"resets at"` with an embedded HH:MM time.
2. A line of `type: "system"` (or `type: "error"`) with an `error.type` of `"rate_limit_error"` or similar marker; body contains the reset time.

`reset_at` is parsed from the message text. If the line matches a "limit hit" pattern but the reset time can't be parsed, fall back to:

```
reset_at = last_activity_at + 5h + 60s
```

The fallback path is logged at debug level.

## Firing a resume

When the poller finds a row where `next_resume_at <= now AND resume_count < resume_cap AND auto_continue = 1`:

1. Derive `session_uuid` from `jsonl_path` (filename stem).
2. Resolve resume prompt: `session.resume_prompt` if non-NULL, else `config.default_resume_prompt`.
3. Build a `SpawnRequest` mirroring the original launch's flags (`model`, `effort`, `permission_mode`, `extra_args`, `terminal_program`) plus `prompt = Some(resume_prompt)` and `resume = Some(session_uuid)`.
4. Call `spawner.spawn(req)`.
5. On success:
   - Insert a **new** session row for the resumed process, with `auto_continue = 1`, `resume_cap` inherited from the head, `resume_count` set to `predecessor.resume_count + 1`, and the same `resume_prompt`.
   - On the predecessor row: set `resumed_into = new_id`, clear `next_resume_at`, set `resume_failures = 0`.
   - Emit `session-changed` and a success toast.
6. On spawn failure:
   - Increment `resume_failures` on the original row.
   - If `resume_failures < 3`: set `next_resume_at = now + 5 min` (back-off, will retry).
   - If `resume_failures >= 3`: clear `next_resume_at`, emit a failure toast, log to debug log. The row remains armed (`auto_continue = 1`) so the user can manually re-toggle to retry.

Per-chain cap: the resumed row inherits `resume_count = predecessor + 1`, so `resume_cap = 3` means three total resumes across the chain. The cap is checked at fire time against the predecessor row.

## App startup recovery

The existing startup path verifies every active row's `claude_pid` against the OS process table. We extend it with **one** rule:

- For any row with `next_resume_at IS NOT NULL AND next_resume_at <= now AND auto_continue = 1 AND resume_count < resume_cap`: run fire-resume immediately.

We deliberately do **not** speculatively resume rows that died without a captured limit event. A row whose process died but had no `next_resume_at` set could have ended for any number of reasons (user kill, crash, normal completion) — auto-resuming on suspicion alone is too aggressive. The user can manually re-launch from history if that's what they wanted.

For rows where the JSONL contained the limit signal but FastClaude was closed during the poller's read window (so `next_resume_at` was never persisted): on next startup the poller runs a tick almost immediately. That tick reads the new JSONL bytes, finds the limit event, and persists `next_resume_at` — at which point the rule above fires it. So this case is covered by the normal detect-then-fire path, not a special startup branch.

## UI

### LaunchDialog

One new row above the Launch button:

```
[ ] Auto-continue when the 5-hour limit resets
    └ resume prompt: [continue                              ] (override default)
```

- Checkbox state defaults to `config.default_auto_continue`.
- Resume-prompt input is collapsed by default; clicking the disclosure reveals a textarea with `config.default_resume_prompt` as placeholder. Empty → use the global default at fire time.
- `resume_cap` is **not** exposed here; it's a Settings-only knob.

### Dashboard session row

Two affordances:

- An "auto-continue" pill (↻ icon + label). Filled when armed, outlined when off. Click toggles `auto_continue`.
- When `next_resume_at` is set, pill shows a live countdown: `↻ 2h 14m`. Tooltip: "Will resume at 14:30 (attempt 2 of 3)."
- When `resume_count >= resume_cap`, pill is muted. Tooltip: "Cap reached — toggle off and on to re-arm."
- For a session that is a resumed continuation, show a small chain indicator linking back to the predecessor row.

### Settings — new "Auto-continue" section

```
Auto-continue defaults
  Default state for new sessions       [ off | on ]
  Default resume prompt                 [ continue                              ]
  Max auto-resumes per session          [ 3 ]   (1–10)
```

Changes apply only to newly-launched sessions; rows already in-flight keep the values stamped at launch.

### Toasts

- **Success at fire time** — "Resumed `<project>` — attempt 2/3" with a "Focus" action.
- **Spawn failure** — "Could not resume `<project>` — `<error>`." Non-fatal; full error in Settings → Logs.
- **Cap reached** — silent (the pill carries the state); only the first fire-attempt-after-cap shows a one-time toast: "Auto-resume cap reached for `<project>`."

## IPC contract additions

**Tauri commands:**

- `set_auto_continue(session_id, on: bool)` — flips the pill from the Dashboard.
- `set_resume_prompt(session_id, prompt: Option<String>)` — saves a per-session override.

The existing `set_config` / `get_config` carry the three new fields without contract change.

**Events:**

- `session-changed` — already exists; emitted on `next_resume_at` set/clear and after fire-resume.

## Error handling

| Scenario | Behavior |
|---|---|
| Reset time unparseable | Fall back to `last_activity_at + 5h + 60s`. Debug-logged. |
| False-positive limit signal | Cap (default 3) bounds damage. User sees cap-reached pill. |
| Spawn fails at fire time | 5-min back-off, up to 3 retries, then give up + toast. Row stays armed in DB. |
| Stale terminal handle | Irrelevant — we spawn a fresh terminal, not focus the old one. |
| Two sessions auto-resume simultaneously | No shared state; each gets its own row. Accepted. |
| User toggles auto-continue off while resume pending | Clear `next_resume_at` immediately. Fire-resume only processes `auto_continue = 1` rows. |
| User kills session manually before reset | Existing dead-PID detection clears `next_resume_at` in the same step (one-line addition to `mark_ended`). |
| FastClaude closed when fire time arrives | Startup recovery fires immediately on next launch. Toast is honest about the delay. |
| Claude CLI not on PATH at fire time | Spawn failure path handles it: back-off, retry, then give up + toast. |

## Testing

### Rust unit tests

- **`usage_reader`** — fixture JSONLs covering (a) an `assistant` rate-limit line with HH:MM reset, (b) a `system`/`error`-typed rate-limit line, (c) a malformed limit line. Assert `limit_event` shape and the fallback timestamp path.
- **`poller`** — fake clock + fake registry + fake spawner. Assert:
  - LimitEvent sets `next_resume_at` only on `auto_continue = 1` rows.
  - Fire-resume triggers exactly when `now >= next_resume_at`.
  - Cap-reached blocks further fires.
  - User-toggled-off rows skip the fire step.
  - Spawn failure back-off and three-strike give-up paths.
- **`session_registry`** — round-trip the new columns; in-memory migration from a pre-existing DB schema.
- **Resume chain accounting** — insert head → fire → assert new row's `resume_count = 1`, cap shared with head; second fire → `resume_count = 2`; third fire → at-cap; fourth attempt skipped.

### Frontend component tests

- LaunchDialog: checkbox initial state, prompt override expand/collapse, default-from-config behavior.
- Dashboard row pill: armed/off/firing/cap-reached visual states; countdown updates against a mock clock.
- Settings: round-trip of the three new fields.

### Manual smoke checklist (Windows for v1)

- Force a real claude session to the 5-hour limit; verify the JSONL contains the expected signal; verify auto-resume fires at the right time and the resumed session loads context.
- Restart FastClaude with a past-due `next_resume_at` → verify startup recovery fires.
- Toggle off mid-pending → verify `next_resume_at` clears.
- User-kill mid-pending → verify clean teardown.

## Open questions deferred to implementation

1. **Exact JSONL signal claude writes on rate-limit.** The plan's first task is capturing one real example on the user's machine (or from a recent session). Line-matching code is written against that sample. Pattern set is then extended if newer Claude versions change the shape.
2. **DB migration mechanics.** Raw `ALTER TABLE ADD COLUMN` statements run from `session_registry::open` after the schema-version check. No external migration crate needed; all new columns have defaults.
3. **Chain visualization in the Dashboard.** The minimum is a "resumed from `<id>`" tooltip on the new row. A fuller tree view is out of scope for v1.
4. **Notification on resume fire when window is hidden.** v1 toasts only. If the user has `launch_mode = hidden`, fire toasts are queued in the toaster but not surfaced as OS notifications. A later iteration can add a native notification path via `tauri-plugin-notification`.

These are implementation details, not design questions; they'll be resolved in the implementation plan.
