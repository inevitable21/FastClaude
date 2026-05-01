# FastClaude Plan 3 — Polished Windows v1.0 Shipping

**Status:** design (pre-plan)
**Date:** 2026-05-01
**Predecessor:** [Plan 2 — polish features](../plans/2026-05-01-fastclaude-plan-2-polish.md)

## Goal

Ship FastClaude as a public Windows-only v1.0 release: an .msi installer
distributed via GitHub Releases with auto-update, a first-run onboarding
experience, clearly labeled wt tabs, helpful errors when the `claude` CLI
isn't on PATH, and CI that verifies every push.

macOS and Linux are deferred — their `Spawner` / `WindowFocus` trait
implementations return a user-visible "not yet supported on this platform"
error. PRs for those backends are welcome but not blocking.

## Non-goals

- macOS / Linux runtime support beyond a friendly "not supported" message
- UI Automation–based tab switching (current behavior: bring window forward;
  user clicks the labeled tab)
- Code-signing certificate (Windows SmartScreen will warn "unknown
  publisher"; the README documents this)
- Telemetry, crash reporting, analytics
- Any cloud / multi-machine session management

## Architecture overview

### Components added or changed

| # | Component | Files |
|---|---|---|
| 1 | App icon | `src-tauri/icons/*` |
| 2 | LICENSE + README | repo root |
| 3 | Labeled wt tabs | `src-tauri/src/spawner/windows.rs` |
| 4 | PATH preflight | `src-tauri/src/spawner/windows.rs` + new `AppError::ClaudeNotOnPath` |
| 5 | First-run onboarding | `src-tauri/src/config.rs`, `src-tauri/src/commands.rs`, new `src/components/Onboarding.tsx`, `src/App.tsx` |
| 6 | Auto-updater | `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, new `src/components/UpdateBanner.tsx`, `src/components/Settings.tsx` |
| 7 | GitHub Actions CI | `.github/workflows/ci.yml` |
| 8 | Release workflow | `.github/workflows/release.yml` |

### Component details

#### 1. App icon

Replace the placeholder Tauri icons in `src-tauri/icons/` with FastClaude
branding. Tauri requires PNGs at `32x32`, `128x128`, `128x128@2x` and a
Windows `icon.ico`. Source: a single 1024×1024 PNG; downscale via Tauri's
`tauri icon` command. The icon is a stylized lightning bolt over a chat
bubble (or whatever the user provides).

#### 2. LICENSE + README

- LICENSE: MIT.
- README sections: what it is (one-paragraph pitch + screenshot), install
  (download `.msi` from releases), first run (onboarding walkthrough), hotkey
  reference + how to change, known limitations (Windows only; unsigned —
  SmartScreen warning expected), build from source (`npm install && npm run
  tauri dev`).

#### 3. Labeled wt tabs

When the configured terminal is `wt` (or auto-resolves to wt), the spawner
adds `--title "FastClaude: <project-folder-name>"` to the wt invocation. The
user sees each session as a distinguishable tab when they bring wt to the
foreground via Focus.

The argv-construction logic in `windows.rs` is extracted into a pure
function `build_wt_argv(project_dir, model, prompt) -> Vec<String>` so it
can be unit-tested without spawning a real process.

#### 4. PATH preflight

Before constructing the spawn command, the Windows spawner checks whether
`claude` resolves on PATH. If not, returns a new typed error
`AppError::ClaudeNotOnPath` whose `Display` impl is:

> `claude` CLI not found on PATH. Install Claude Code from
> https://docs.claude.com/en/docs/claude-code/setup, then restart FastClaude.

`LaunchDialog.tsx` already toasts the error message verbatim, so no frontend
work is needed beyond verifying the message renders cleanly.

The lookup is wrapped behind a small trait:

```rust
trait PathLookup: Send + Sync {
    fn find(&self, exe: &str) -> Option<PathBuf>;
}
```

so the preflight test can inject a fake.

#### 5. First-run onboarding

`config::load` is split:

```rust
pub fn load(path: &PathBuf) -> AppResult<(Config, bool /* was_created */)>;
```

Callers updated. `AppState` gains `is_first_run: AtomicBool`, set true when
`was_created` is true. New IPC commands:

```
get_first_run() -> bool
clear_first_run() -> ()
```

`App.tsx` checks `get_first_run()` on mount; if true, renders
`<Onboarding />` instead of `<Dashboard />`. Onboarding is a single screen
with three fields (terminal, hotkey, default model) plus a "Get started"
button that calls `set_config` then `clear_first_run`, then the dashboard
appears.

#### 6. Auto-updater

Adds the `tauri-plugin-updater` plugin and a public signing key in
`tauri.conf.json`:

```json
"plugins": {
  "updater": {
    "endpoints": [
      "https://github.com/<owner>/FastClaude/releases/latest/download/latest.json"
    ],
    "pubkey": "<base64-public-key>"
  }
}
```

Backend additions in `commands.rs`:

```
check_for_update() -> Option<UpdateInfo { version, notes }>
install_update()   -> ()   // downloads, applies, restarts
```

Frontend additions:

- `<UpdateBanner />` in `App.tsx` — renders when `check_for_update` returns
  Some; click "Restart to install" → `install_update`.
- A "Check for updates" button in `Settings.tsx` runs the same check
  on demand and toasts the result either way.

The startup check fires once, ~5 seconds after launch (don't block the
window). On network failure, swallow the error silently — retry next
launch.

#### 7. CI workflow (`ci.yml`)

Triggers: push to `develop` or `main`, every pull request.

Jobs (single matrix entry, `windows-latest`):

```yaml
- checkout
- setup-node 20
- setup-rust stable
- npm ci
- npm run build         # tsc + vite
- cargo test --manifest-path src-tauri/Cargo.toml
- npm run tauri build   # produces .msi but does not upload
```

#### 8. Release workflow (`release.yml`)

Triggers: git tag matching `v*`.

```yaml
- checkout
- setup-node 20
- setup-rust stable
- npm ci
- env:
    TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
  run: npm run tauri build
- generate latest.json from src-tauri/target/release/bundle/msi/*.msi.sig
- create draft GitHub Release with .msi + latest.json attached
```

The maintainer manually publishes the draft after sanity-checking. Once
published, existing installs see it on their next startup.

## Data flow — first run

```
launch app
  → main.rs: config::load returns (cfg, was_created=true)
  → AppState.is_first_run := true
  → frontend mounts, calls get_first_run() → true
  → renders <Onboarding>
  → user fills form, clicks Get started
  → frontend calls set_config(filled) then clear_first_run()
  → AppState.is_first_run := false
  → frontend re-renders <Dashboard>
```

Subsequent launches: `was_created=false`, `is_first_run` stays false,
dashboard renders directly.

## Data flow — update

```
launch app
  → 5s timer
  → check_for_update() → Some(UpdateInfo) | None
  → if Some: render <UpdateBanner>
  → user clicks Restart to install
  → install_update() → plugin downloads, verifies signature, applies,
    relaunches the app
```

## Error handling

- `claude` not on PATH → `AppError::ClaudeNotOnPath` with install URL in
  message; surfaced as toast via existing LaunchDialog error path
- Spawner / focus called on macOS or Linux → `AppError::PlatformUnsupported`
  with message "FastClaude doesn't yet support {macOS|Linux}; contributions
  welcome at <repo URL>"
- Update check network failure → silent (logged via `eprintln!`); next
  startup retries
- Update install signature mismatch → Tauri plugin returns error; toast
  shown; do not relaunch

## Testing strategy

### Automated

Existing `cargo test` (23 tests) keeps running. New unit tests:

- `spawner/windows.rs::tests::wt_argv_includes_title` — `build_wt_argv` puts
  `--title FastClaude: <name>` in the right position
- `spawner/windows.rs::tests::preflight_returns_typed_error_when_missing` —
  with a fake `PathLookup` returning `None`, spawn returns
  `ClaudeNotOnPath`
- `config::tests::load_signals_first_run_on_create` — fresh dir → `(_, true)`
- `config::tests::load_signals_not_first_run_on_existing` — pre-existing
  config → `(_, false)`

CI also runs `npm run build` and `npm run tauri build` (catches bundle /
icon / config regressions).

### Manual smoke (run before tagging a release)

Run on a Windows machine (or VM) with no prior FastClaude state.

1. Fresh install (wipe `%APPDATA%\com.fastclaude.app\`) — Onboarding
   appears, completes, Dashboard shows
2. Launch a session in wt — tab title shows `FastClaude: <project>`
3. Temporarily remove `claude` from PATH (e.g. `cmd /v` rename) — Launch
   dialog shows the specific error toast with the install URL
4. Tag a throwaway `v0.1.0-test` — `release.yml` builds and uploads .msi +
   latest.json to a draft release
5. Install the .msi from that release — app runs (SmartScreen warning
   appears once; click "Run anyway")
6. Bump version, push `v0.1.1-test` tag — installed app detects update on
   next launch, prompts, installs, restarts
7. Verify icon: installer, taskbar, Settings page, .msi properties

### Out of test coverage

- macOS / Linux runtime — only the "not supported" toast is exercised
- UIA tab switching — out of scope
- Update signature forgery — trust the plugin
- Telemetry / crash reporting — not present

## Open decisions for the implementation plan

- Repository owner string for updater endpoint (decide before writing
  `release.yml`)
- App icon source (user provides, or generate placeholder for v1.0?)

These are flagged in the plan so the implementer surfaces them at the
relevant step rather than guessing.

## Spec coverage check

- [x] App icon
- [x] LICENSE + README
- [x] Labeled wt tabs (option B from brainstorming)
- [x] PATH preflight + typed error
- [x] First-run onboarding
- [x] Auto-updater via tauri-plugin-updater + GitHub Releases
- [x] CI workflow
- [x] Release workflow
- [x] macOS / Linux deferral with user-visible message
- [x] Test strategy (automated + manual)
