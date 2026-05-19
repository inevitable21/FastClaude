# Projects, TODOs, and Auto-Split Session Management — Design Spec

**Date:** 2026-05-19
**Status:** Draft, awaiting user review

## Summary

FastClaude becomes project-aware. Every folder you've ever launched a session in is a first-class **Project** in a left sidebar (rename / pin / hide). Each project has its own **TODOs** with **Ongoing** and **Finished** tabs. You add a TODO as a one-liner; FastClaude calls `claude -p` to **decompose it into subtasks**, shows you the result for review (edit, reorder, delete, manual-add, re-plan), and on **Launch all** spawns one Claude Code session per subtask using today's launcher. Each session knows the parent TODO and is badged accordingly.

## Goals

- Make multi-project, multi-session work easy to manage from one screen.
- First-class **Project** entity with rename / pin / hide (zero forced setup — auto-created from launch history).
- TODOs per project with **Pending → Ongoing → Finished** lifecycle.
- A single TODO can be **split into subtasks via `claude -p`** and each subtask launched as its own Claude Code session.
- Always show the planner output before launching anything — no surprise sessions.
- Sessions remain visible in the main dashboard; project filtering is additive, not replacement.

## Non-goals (v1)

- Cross-project TODOs (each TODO belongs to exactly one project).
- Sequential subtask queueing (decided: parallel-via-review-then-launch).
- Editing the planner prompt template from Settings (constant in code; revisit in v2).
- Importing markdown TODO lists from disk.
- Cross-device sync.
- Auto-finishing TODOs without user confirmation (a session ending does not equal a task done).

## Architecture

Three new Rust modules, mirroring `session_registry.rs`. Existing modules gain one column each. No changes to spawner, poller, or window-focus internals.

```
┌─ React (Tauri commands) ─────────────────────────────┐
│  Projects sidebar  TODOs panel  Sessions panel       │
└──────────┬───────────────┬───────────────┬───────────┘
           ▼               ▼               ▼
       projects.rs ◀─── todos.rs ───▶ session_registry.rs
                            │              ▲
                            ▼              │
                        planner.rs ────────┘
                            │   (returns Vec<String>;
                            ▼    caller writes subtasks
                       claude -p     and spawns sessions)
```

- **`projects.rs`** — `Projects` registry over a new `projects` table. CRUD: `upsert_for_path`, `list`, `set_display_name`, `set_pinned`, `set_hidden`, `get`. Path normalization reuses `session_registry::normalize_project_dir`.
- **`todos.rs`** — `Todos` registry over `todos` + `subtasks` tables. CRUD for both. Aggregate helpers: `list_for_project(project_id, state_filter)`, `aggregate_state(todo_id)`, `attach_session(subtask_id, session_id)`.
- **`planner.rs`** — pure module. `plan_subtasks(title, project_name, model) -> AppResult<Vec<String>>`. Spawns `claude -p`, captures stdout with a 60s timeout, parses JSON. No DB writes; the caller persists.

Module boundaries match the existing poller / spawner / registry split: planning knows nothing about DB; todos knows nothing about spawning; commands wires them together.

## Data model

### New table `projects`

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | UUID |
| `norm_path` | TEXT UNIQUE NOT NULL | output of `normalize_project_dir`; one row per folder |
| `display_name` | TEXT NOT NULL | defaults to last path segment; user-editable |
| `pinned` | INTEGER (bool) NOT NULL DEFAULT 0 | sorts to top in sidebar |
| `hidden` | INTEGER (bool) NOT NULL DEFAULT 0 | hides from sidebar without deleting TODOs |
| `created_at` | INTEGER NOT NULL | epoch s |

Auto-created on first `launch_session` and on first manual TODO add. The existing `recent_projects` lookup continues to work: the sidebar shows the union of `(projects WHERE hidden = 0)` and recent Claude folders not yet promoted to `projects` (those are rendered ghosted; clicking promotes them to a full row).

### New table `todos`

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | UUID |
| `project_id` | TEXT FK NOT NULL | |
| `title` | TEXT NOT NULL | what the user typed |
| `state` | TEXT NOT NULL | `pending` / `ongoing` / `finished` (see derivation below) |
| `planner_status` | TEXT NOT NULL | `idle` / `planning` / `planned` / `planner_failed` |
| `planner_error` | TEXT NULL | last error message if `planner_status = planner_failed` |
| `auto_suggest_done_at` | INTEGER NULL | epoch s when every child session ended; drives "Done?" badge |
| `created_at` | INTEGER NOT NULL | |
| `completed_at` | INTEGER NULL | set when user clicks "Mark finished" |

### New table `subtasks`

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | UUID |
| `todo_id` | TEXT FK NOT NULL | cascade-delete with parent TODO |
| `ord` | INTEGER NOT NULL | renumbered on reorder |
| `text` | TEXT NOT NULL | becomes the session's launch prompt verbatim |
| `session_id` | TEXT NULL | set when launched |
| `origin` | TEXT NOT NULL | `planner` / `manual` |
| `created_at` | INTEGER NOT NULL | |

### `sessions` table — one new column

| Column | Type | Default | Notes |
|---|---|---|---|
| `subtask_id` | TEXT NULL | NULL | back-pointer for "▣ Sessions UI redesign • 2/3" badge |

Added with `ALTER TABLE ADD COLUMN`, same migration pattern as the auto-continue columns.

### TODO state derivation

`state` is stored on the row but recomputed by `todos.rs::aggregate_state` on every write that could change it (subtask launch, session-ended event, manual mark-finished). It is not stored independently of evidence. Definitions, evaluated top-down — first match wins:

1. `finished` — user clicked "Mark finished". `completed_at` is set.
2. `ongoing` — at least one subtask has a `session_id` whose session is **not** `ended`.
3. `pending` — everything else. This covers: no subtasks launched yet, or all launched sessions have ended without confirmation.

`auto_suggest_done_at` is set when a state recomputation observes that every subtask has a `session_id` and every one of those sessions is `ended`. It surfaces a "Done?" badge on the TODO row — clicking "Mark finished" transitions to `finished`; "Not yet" clears `auto_suggest_done_at` (TODO stays `pending` per rule 3, and the badge won't re-appear unless new sessions launch and then end).

## Planner flow

`planner.rs::plan_subtasks` is a pure function:

```rust
pub fn plan_subtasks(
    title: &str,
    project_name: &str,
    model: &str,
) -> AppResult<Vec<String>>
```

### Steps

1. Build prompt from a constant template (see "Planner prompt" below).
2. Spawn `claude -p <prompt> --model <model> --output-format json`. Working directory = system temp (so the planner can't accidentally read project files).
3. `tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())`. Kill on timeout.
4. Parse stdout as JSON. Expect `{"subtasks": ["...", "..."]}`. Reject:
   - non-JSON output → `AppError::PlannerFailed("planner returned non-JSON: <first 200 chars>")`
   - missing `subtasks` key → `AppError::PlannerFailed("missing 'subtasks' key")`
   - array length not in 1..=8 → `AppError::PlannerFailed("expected 1-8 subtasks, got N")`
   - any item empty or >500 chars → `AppError::PlannerFailed("subtask N invalid length")`
5. Return `Vec<String>`.

### Planner prompt template (constant in code)

```
You are a planning assistant. Decompose the following TODO into 2 to 5 concrete
subtasks that can each be worked on independently by a separate Claude Code
session. Each subtask must be a self-contained instruction (no cross-references
between subtasks). Reply with strict JSON only.

Project: <project_name>
TODO: <title>

Reply format:
{"subtasks": ["...", "...", "..."]}
```

### Subprocess abstraction for testability

Trait `PlannerRunner { fn run(&self, args: &[&str]) -> AppResult<String>; }`. `RealRunner` shells out; `FakeRunner` returns scripted strings. Same pattern as `Spawner` / `FakeSpawner` already used in the codebase. Tests exercise the parser against fixed stdout strings; no real `claude` binary required.

### `plan_todo` tauri command (orchestrator)

```
plan_todo(todo_id):
  guard: if todo_id ∈ in_flight_planning_set → AppError::Invalid("already planning")
  in_flight_planning_set.insert(todo_id)
  registry.set_planner_status(todo_id, "planning")
  emit("todo-changed", todo_id)
  result = spawn_blocking(|| planner::plan_subtasks(title, project_name, model))
  on success:
    todos::replace_subtasks(todo_id, planner_origin = "planner", strings)
    registry.set_planner_status(todo_id, "planned")
  on failure:
    registry.set_planner_status(todo_id, "planner_failed")
    registry.set_planner_error(todo_id, error_string)
  in_flight_planning_set.remove(todo_id)
  emit("todo-changed", todo_id)
```

### Re-plan rule

A repeat `plan_todo` call replaces all existing subtasks **iff none of them have a `session_id`**. If any subtask has been launched, the command returns `AppError::Invalid("cannot re-plan after sessions launched")`. Re-planning is for "the planner output was bad and I haven't launched anything yet."

## UI flow (frontend)

### Top-level shell changes

`App.tsx` gains a fourth view value (`"projects"`) and that becomes the default landing view (today's `"dashboard"` is reachable via "All sessions" in the sidebar).

State in `App.tsx`:

```ts
type View = "projects" | "dashboard" | "history" | "settings" | "onboarding";
const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
// null = "All sessions" virtual entry
```

### New components

- **`ProjectSidebar.tsx`** — left rail.
  - Sorted: pinned (alphabetical) → unpinned (last-touched desc) → ghosted (auto-discovered folders not yet promoted).
  - Row hover: ⋯ menu with Rename / Pin-Unpin / Hide / Open in launcher.
  - Footer: "+ Add project" (folder picker; calls `upsert_project`) and, if any hidden projects exist, a `Show hidden (N)` toggle that reveals them in a separate dimmed section with an `Unhide` action per row.
  - Top row: "★ All sessions" (selects `selectedProjectId = null`).

- **`ProjectPane.tsx`** — main content when a project is selected.
  - Header: project name (inline editable), badge of active session count, "+ TODO" and "+ Launch session" buttons (Launch session pre-fills `project_dir`).
  - `<TodoList projectId>`
  - `<SessionsForProject projectId>` (reuses existing `SessionRow`)

- **`TodoList.tsx`** — tabs Ongoing / Finished. Each row:
  - Title, state pill (`pending` / `ongoing` / `finished`)
  - Subtask count summary: "3 sessions • 2 running"
  - "Done?" badge if `auto_suggest_done_at` is set (clicking opens a small confirm: "Mark finished" / "Not yet")
  - Click row → expands to show subtasks + their session statuses

- **`TodoDialog.tsx`** — "+ TODO" entry. Single text input → Save → calls `create_todo` then immediately `plan_todo`. Stays open showing a spinner; when `todo-changed` reports `planner_status = planned`, transitions into `SubtaskReviewDialog`. On `planner_failed`, shows the error with "Try again" / "Save without planning".

- **`SubtaskReviewDialog.tsx`** — opens when planner finishes successfully.
  - Reorderable list (drag handle, native HTML5 DnD — no new dep).
  - Each row: drag handle, editable text (textarea autosize), delete button.
  - Footer: "+ Add manual subtask", "Re-plan" (disabled if any subtask already has `session_id`), per-row "Launch", and "Launch all".
  - "Launch all" calls one tauri command (`launchAllSubtasks`) that loops in the backend spawning each subtask via the existing `launch_session` path (subtask `text` becomes the launch `prompt`). The loop is server-side to avoid N concurrent IPC round-trips from the frontend; spawns happen in quick succession — there is no inter-spawn delay or wait-for-previous-to-finish. If any spawn fails, the loop stops, the error surfaces, and remaining subtasks stay un-launched.

- **`SessionRow.tsx`** — gains optional `<ParentBadge>`. When `session.subtask_id` is set, the badge shows "▣ <todo title truncated> • <i>/<N>" where i is the subtask's `ord` (1-indexed) and N is the total subtask count for that TODO. Clicking the badge navigates to the parent TODO in the project view.

### IPC additions (`src/lib/ipc.ts`)

Commands:
- `listProjects()`, `upsertProject(path)`, `setProjectName(id, name)`, `setProjectPinned(id, on)`, `setProjectHidden(id, on)`, `deleteProject(id)`
- `listTodos(projectId, stateFilter?)`, `createTodo(projectId, title)`, `markTodoFinished(id)`, `dismissAutoSuggest(id)`, `deleteTodo(id, killRunningSessions)`
- `planTodo(todoId)` (orchestrator)
- `reorderSubtasks(todoId, orderedIds)`, `editSubtask(id, text)`, `deleteSubtask(id)`, `addManualSubtask(todoId, text)`
- `launchSubtask(subtaskId, launchOverrides?)`, `launchAllSubtasks(todoId, launchOverrides?)`

Events:
- `onProjectChanged((id) => ...)` — emitted from project CRUD
- `onTodoChanged((id) => ...)` — emitted from todo / subtask CRUD and from session state changes that affect TODO aggregate state

### Today's existing surfaces

- `Dashboard` and `History` keep working as-is — both gain an optional project-name column for context.
- `LaunchDialog` is unchanged. Launching from a project pre-fills `projectDir`.
- The hotkey still opens `LaunchDialog`. Project context is not required to launch.

## Error handling

- **Planner timeout / parse failure** — TODO ends in `planner_failed`, `planner_error` populated. UI shows inline error and "Try again". No subtasks written.
- **`claude -p` not on PATH** — same path as today's missing-claude detection. Toast: "claude CLI not found — install it or check PATH".
- **Session launch from subtask fails** — subtask keeps `session_id = NULL`; UI row shows error pill; TODO state recomputes (likely returns to `pending` if no other subtask launched successfully).
- **User deletes a project that has TODOs or non-ended sessions** — refused. Confirmation modal: "This project has N TODOs and M sessions. Hide instead?" Default action is Hide. Delete is allowed only when no TODOs and no non-ended sessions remain.
- **User deletes a TODO with running sessions** — confirmation modal: "Kill N running sessions?" Yes → kill each via existing `kill_session`. No → cancel.
- **Concurrent re-plan after launch** — refused at the command layer (see "Re-plan rule" above).
- **Concurrent `plan_todo` for the same TODO** — refused with `AppError::Invalid("already planning")`.

## Testing

Backend tests follow the existing `#[cfg(test)] mod tests` pattern (in-memory SQLite, no real `claude` process).

- **`projects.rs`** — upsert idempotency under path-normalization variants (`C:\X` ≡ `c:/x/`), pin/hide round-trip, rename persistence, list ordering (pinned first then last-touched desc).
- **`todos.rs`** — CRUD round-trip; `aggregate_state` returns `ongoing` while any child session is non-ended and `pending` once all are ended (the auto-suggest surface, not auto-finished); subtask reorder renumbers `ord`; cascade-delete subtasks when parent TODO is deleted; `attach_session` is idempotent.
- **`planner.rs`** — JSON parser unit tests: happy path, empty array rejected, >8 items rejected, >500-char item rejected, missing key rejected, non-JSON rejected. Subprocess flow tested via `FakeRunner`.
- **`commands.rs`** — `plan_todo` guard against double-fire; re-plan rejected when any subtask has `session_id`; project-delete refused with non-ended sessions; `launchAllSubtasks` is sequential and stops on the first launch error (subsequent subtasks remain un-launched, TODO state recomputes correctly).
- **Migration** — open a legacy DB without `projects` / `todos` / `subtasks` / `sessions.subtask_id`; assert reads still work, tables and columns created, existing session rows readable.

Frontend has no test infra today; this change does not add it. A short manual test plan will live alongside the implementation plan.

## Open questions (none blocking — defaults below)

- **Planner model** — defaults to `config.default_model`. Could later add a `planner_model` config field; v1 reuses the default to avoid yet another Settings row.
- **Planner timeout** — 60s. Made a constant; reconsider if real usage shows it's too short for Opus.
- **Subtask count bounds** — 1..=8 stored, prompt asks for 2..=5. The looser bound at parse time lets the model occasionally return 1 or 6-8 without failing the whole TODO.

---

## Manual test plan (for the implementation phase)

1. Fresh install → onboarding still works → land on the new Projects view → sidebar empty except "★ All sessions" → "+ Add project" picker round-trips.
2. Launch a session in a new folder → folder appears in the sidebar as a non-pinned project, named after the folder's leaf segment.
3. Add a TODO in a project → planner spinner → review dialog opens with planner subtasks.
4. In review: edit one subtask, delete another, manual-add a third, drag-reorder. Click "Re-plan" — refused as soon as anything has been launched, allowed before.
5. "Launch all" → N terminals open, all labeled correctly. Each session row in the dashboard shows the parent-TODO badge with the right ordinal.
6. TODO appears in the Ongoing tab. Close one session → "Done?" badge does not appear yet. Close all sessions → "Done?" badge appears. Click "Not yet" → TODO returns to Pending. Trigger again, click "Mark finished" → TODO moves to Finished tab with `completed_at` set.
7. Delete a project with TODOs → blocked with the "Hide instead?" modal. Hide → project disappears from the main sidebar list. Confirm the sidebar footer's "Show hidden (N)" toggle reveals it; clicking Unhide restores the row. Sessions in a hidden project are still listed under "All sessions".
8. Migration: install over an existing v1.x build with active sessions and history → all sessions still listed, no projects yet → launching in an old folder creates its project row.
