# Launch on startup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users opt FastClaude into launching at Windows login, with a Settings-controlled launch mode (Window / Minimized / Hidden).

**Architecture:** Add `tauri-plugin-autostart`. Store two new fields on `Config`. On Settings save, call `AutoLaunchManager.enable()` or `.disable()`. On every boot, detect autostart-launch via a `--launched-by-autostart` CLI flag and apply the mode (read from config) to the main window.

**Tech Stack:** Rust (tauri 2.x, `tauri-plugin-autostart` 2.x), TypeScript/React (frontend Settings UI).

**Spec:** `docs/superpowers/specs/2026-05-17-launch-on-startup-design.md`

---

## File map

| File | Action | Responsibility |
|---|---|---|
| `src-tauri/Cargo.toml` | modify | Add `tauri-plugin-autostart` dep |
| `src-tauri/capabilities/default.json` | modify | Add `autostart:default` capability |
| `src-tauri/src/lib.rs` | modify | Register new `launch_args` module |
| `src-tauri/src/launch_args.rs` | **create** | CLI flag parser (`parse_launch_args`) |
| `src-tauri/src/config.rs` | modify | Add `LaunchMode` enum, two `Config` fields, defaults, tests |
| `src-tauri/src/commands.rs` | modify | `set_config` calls `reconcile_autostart` before saving |
| `src-tauri/src/autostart.rs` | **create** | `reconcile_autostart(&Config, &AppHandle)` — dev-mode no-op |
| `src-tauri/src/main.rs` | modify | Register plugin; parse CLI args in `.setup()`; apply window mode; path-drift warn |
| `src-tauri/tauri.conf.json` | modify | Window default `"visible": false` |
| `src/types.ts` | modify | Extend `AppConfig` with two new fields |
| `src/components/Settings.tsx` | modify | Add `Startup` section UI |

---

## Task 1: Add autostart plugin dependency and capability

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the Rust dependency**

In `src-tauri/Cargo.toml`, locate the `[dependencies]` block (currently ends at line 33 with `tauri-plugin-updater = "2.10.1"`). Add this line immediately after `tauri-plugin-updater`:

```toml
tauri-plugin-autostart = "2"
```

- [ ] **Step 2: Add the capability permission**

In `src-tauri/capabilities/default.json`, the `permissions` array currently ends with `"updater:default"`. Add a comma after `"updater:default"` and a new entry on the next line:

```json
"autostart:default"
```

The full `permissions` array should now be:

```json
"permissions": [
  "core:default",
  "core:window:allow-minimize",
  "core:window:allow-toggle-maximize",
  "core:window:allow-close",
  "core:window:allow-start-dragging",
  "core:window:allow-is-maximized",
  "opener:default",
  "global-shortcut:allow-register",
  "global-shortcut:allow-unregister",
  "global-shortcut:allow-is-registered",
  "updater:default",
  "autostart:default"
]
```

- [ ] **Step 3: Verify the crate resolves**

Run from the repo root:

```
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: completes successfully (will download the new crate on first run; may take 30–60s).

- [ ] **Step 4: Commit**

```
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/capabilities/default.json
git commit -m "feat(autostart): add tauri-plugin-autostart dependency and capability"
```

---

## Task 2: Add `LaunchMode` enum (TDD)

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/config.rs`, inside the existing `#[cfg(test)] mod tests` block (after the last test, before the closing `}`), add:

```rust
#[test]
fn launch_mode_serde_round_trip() {
    use serde_json::json;
    for m in [LaunchMode::Window, LaunchMode::Minimized, LaunchMode::Hidden] {
        let s = serde_json::to_value(&m).unwrap();
        let back: LaunchMode = serde_json::from_value(s.clone()).unwrap();
        assert_eq!(m, back, "round-trip failed for {:?} (serialized as {})", m, s);
    }
}

#[test]
fn launch_mode_serializes_as_snake_case() {
    assert_eq!(serde_json::to_string(&LaunchMode::Window).unwrap(), "\"window\"");
    assert_eq!(serde_json::to_string(&LaunchMode::Minimized).unwrap(), "\"minimized\"");
    assert_eq!(serde_json::to_string(&LaunchMode::Hidden).unwrap(), "\"hidden\"");
}

#[test]
fn launch_mode_default_is_window() {
    assert_eq!(LaunchMode::default(), LaunchMode::Window);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib launch_mode
```

Expected: FAIL with "cannot find type `LaunchMode` in this scope" (compile error).

- [ ] **Step 3: Add the enum to `config.rs`**

In `src-tauri/src/config.rs`, between the `use` imports (around line 3) and the `Config` struct (around line 5), add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Window,
    Minimized,
    Hidden,
}

impl Default for LaunchMode {
    fn default() -> Self {
        LaunchMode::Window
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib launch_mode
```

Expected: PASS for all three tests.

- [ ] **Step 5: Commit**

```
git add src-tauri/src/config.rs
git commit -m "feat(config): add LaunchMode enum"
```

---

## Task 3: Add `parse_launch_args` (TDD)

**Files:**
- Create: `src-tauri/src/launch_args.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Register the new module**

In `src-tauri/src/lib.rs`, add a new `pub mod` line so the file becomes:

```rust
pub mod commands;
pub mod config;
pub mod error;
pub mod launch_args;
pub mod poller;
pub mod recent_projects;
pub mod session_registry;
pub mod spawner;
pub mod usage_reader;
pub mod window_focus;
```

- [ ] **Step 2: Create the file with the failing tests only**

Create `src-tauri/src/launch_args.rs` with this content:

```rust
//! Parses launch-time CLI flags injected by the autostart plugin.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchArgs {
    pub from_autostart: bool,
}

pub fn parse_launch_args<I, S>(args: I) -> LaunchArgs
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    todo!("not yet implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flag_means_manual_launch() {
        let args = parse_launch_args(["fastclaude.exe"]);
        assert!(!args.from_autostart);
    }

    #[test]
    fn flag_present_means_autostart_launch() {
        let args = parse_launch_args(["fastclaude.exe", "--launched-by-autostart"]);
        assert!(args.from_autostart);
    }

    #[test]
    fn flag_anywhere_in_args_is_detected() {
        let args = parse_launch_args([
            "fastclaude.exe",
            "--some-other-flag",
            "--launched-by-autostart",
            "extra",
        ]);
        assert!(args.from_autostart);
    }

    #[test]
    fn unrelated_args_are_ignored() {
        let args = parse_launch_args(["fastclaude.exe", "--unrelated", "--launched-by-something-else"]);
        assert!(!args.from_autostart);
    }

    #[test]
    fn empty_args_is_manual_launch() {
        let empty: [&str; 0] = [];
        let args = parse_launch_args(empty);
        assert!(!args.from_autostart);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib launch_args
```

Expected: tests compile; all panic at the `todo!()` macro.

- [ ] **Step 4: Implement `parse_launch_args`**

Replace the `todo!("not yet implemented")` body with:

```rust
    let mut from_autostart = false;
    for a in args {
        if a.as_ref() == "--launched-by-autostart" {
            from_autostart = true;
        }
    }
    LaunchArgs { from_autostart }
```

- [ ] **Step 5: Run tests to verify they pass**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib launch_args
```

Expected: all 5 tests PASS.

- [ ] **Step 6: Commit**

```
git add src-tauri/src/launch_args.rs src-tauri/src/lib.rs
git commit -m "feat(autostart): add parse_launch_args CLI flag parser"
```

---

## Task 4: Extend `Config` with the two new fields

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: Write the failing test for legacy-config defaulting**

In `src-tauri/src/config.rs`, inside the `#[cfg(test)] mod tests` block, add this test alongside the existing `load_defaults_prompt_to_empty_when_missing` test:

```rust
#[test]
fn load_defaults_autostart_fields_when_missing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("c.json");
    std::fs::write(
        &path,
        br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
            "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
    )
    .unwrap();
    let (cfg, _) = load(&path).unwrap();
    assert!(!cfg.launch_on_login, "launch_on_login must default to false");
    assert_eq!(cfg.launch_mode, LaunchMode::Window);
}

#[test]
fn config_default_has_autostart_off() {
    let cfg = Config::default();
    assert!(!cfg.launch_on_login);
    assert_eq!(cfg.launch_mode, LaunchMode::Window);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib config
```

Expected: FAIL — compile errors `no field 'launch_on_login' on Config` / `no field 'launch_mode' on Config`.

- [ ] **Step 3: Add the fields to the `Config` struct**

In `src-tauri/src/config.rs`, inside the `Config` struct (currently ending at line 27 with `pub default_prompt: String,`), add two more fields just before the closing `}`:

```rust
    /// If true, register FastClaude to launch at Windows login.
    #[serde(default)]
    pub launch_on_login: bool,
    /// How the window should appear when launched by autostart.
    /// (Read on every boot from the loaded config.)
    #[serde(default)]
    pub launch_mode: LaunchMode,
```

- [ ] **Step 4: Update `Default::default()` to initialize them**

In the `impl Default for Config` block (currently ending around line 41 with `default_prompt: String::new(),`), add inside the struct literal before the closing `}`:

```rust
            launch_on_login: false,
            launch_mode: LaunchMode::Window,
```

- [ ] **Step 5: Run tests to verify they pass**

```
cargo test --manifest-path src-tauri/Cargo.toml --lib config
```

Expected: all `config` tests PASS, including the two new ones and all the pre-existing ones.

- [ ] **Step 6: Extend the TypeScript `AppConfig` interface**

In `src/types.ts`, replace the existing `AppConfig` interface (lines 29–38) with:

```ts
export type LaunchMode = "window" | "minimized" | "hidden";

export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
  default_prompt: string;
  launch_on_login: boolean;
  launch_mode: LaunchMode;
}
```

- [ ] **Step 7: Verify TypeScript compiles**

```
npm run build
```

Expected: `tsc` step passes (no type errors); vite build completes. (This may emit warnings about the dist dir; that's fine.)

- [ ] **Step 8: Commit**

```
git add src-tauri/src/config.rs src/types.ts
git commit -m "feat(config): add launch_on_login and launch_mode fields"
```

---

## Task 5: Add `reconcile_autostart` and call it from `set_config`

**Files:**
- Create: `src-tauri/src/autostart.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Create the autostart helper module**

Create `src-tauri/src/autostart.rs` with this content:

```rust
//! Reconciles the OS autostart registry entry with the user's saved config.
//!
//! Called from `set_config` on every settings save. Returns the underlying
//! plugin error (boxed as `anyhow`) so it can surface as a toast in the UI.

use crate::config::Config;
use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// Production behavior: enable or disable the registry entry based on `cfg`.
#[cfg(not(debug_assertions))]
pub fn reconcile_autostart<R: Runtime>(app: &AppHandle<R>, cfg: &Config) -> AppResult<()> {
    let manager = app.autolaunch();
    if cfg.launch_on_login {
        manager
            .enable()
            .map_err(|e| AppError::Other(format!("enable autostart: {e}")))?;
    } else {
        manager
            .disable()
            .map_err(|e| AppError::Other(format!("disable autostart: {e}")))?;
    }
    Ok(())
}

/// Dev-mode no-op: developer machines should never see registry writes from
/// `cargo run` / `tauri dev`. The Settings toggle still persists to config;
/// only the OS-level effect is skipped.
#[cfg(debug_assertions)]
pub fn reconcile_autostart<R: Runtime>(_app: &AppHandle<R>, _cfg: &Config) -> AppResult<()> {
    Ok(())
}
```

- [ ] **Step 2: Confirm `AppError::Other(String)` exists**

Open `src-tauri/src/error.rs` and check the `AppError` enum.

If a variant matching `Other(String)` (or similar — `Msg(String)`, `Generic(String)`, etc.) already exists, adjust the two `AppError::Other(...)` lines in `autostart.rs` to use the actual variant name.

If no string variant exists, add one to `error.rs` by inserting (inside the `AppError` enum, before the closing `}`):

```rust
    #[error("{0}")]
    Other(String),
```

(`thiserror::Error` is already in use per `Cargo.toml`.)

- [ ] **Step 3: Register the new module**

In `src-tauri/src/lib.rs`, add `pub mod autostart;` to the module list (alphabetic order):

```rust
pub mod autostart;
pub mod commands;
pub mod config;
pub mod error;
pub mod launch_args;
pub mod poller;
pub mod recent_projects;
pub mod session_registry;
pub mod spawner;
pub mod usage_reader;
pub mod window_focus;
```

- [ ] **Step 4: Wire `reconcile_autostart` into `set_config`**

In `src-tauri/src/commands.rs`, locate the `set_config` command (currently lines 219–227):

```rust
#[tauri::command]
pub fn set_config(state: State<'_, AppState>, cfg: Config) -> AppResult<()> {
    {
        let mut held = state.config.lock().unwrap();
        *held = cfg.clone();
    }
    config::save(&state.config_path, &cfg)?;
    Ok(())
}
```

Replace it with:

```rust
#[tauri::command]
pub fn set_config(app: tauri::AppHandle, state: State<'_, AppState>, cfg: Config) -> AppResult<()> {
    // Reconcile OS-level autostart BEFORE persisting. If this fails we don't
    // want a config file that says "on" while the registry says "off".
    crate::autostart::reconcile_autostart(&app, &cfg)?;
    config::save(&state.config_path, &cfg)?;
    let mut held = state.config.lock().unwrap();
    *held = cfg;
    Ok(())
}
```

- [ ] **Step 5: Register the autostart plugin in `main.rs`**

In `src-tauri/src/main.rs`, locate the `tauri::Builder::default()` chain (around lines 16–19):

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
```

Add the autostart plugin after the updater plugin:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new()
            .args(["--launched-by-autostart"])
            .build())
```

(The `Builder::new().args(...).build()` form lets us register the static CLI flag that every autostart launch will pass.)

If the builder API differs in this version of the crate (older versions used `tauri_plugin_autostart::init(MacosLauncher, Option<Vec<&str>>)`), use the `init`-function form instead:

```rust
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--launched-by-autostart"]),
        ))
```

Use whichever the crate version exposes — `cargo check` in the next step will reveal which.

- [ ] **Step 6: Verify everything compiles**

```
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: clean compile. If `tauri_plugin_autostart::Builder` is not found, switch to the `init(...)` form per the previous step.

- [ ] **Step 7: Run all tests**

```
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all tests PASS (no regressions in pre-existing tests). The new code has no unit tests of its own — `reconcile_autostart` is an OS integration verified manually in Task 8.

- [ ] **Step 8: Commit**

```
git add src-tauri/src/autostart.rs src-tauri/src/lib.rs src-tauri/src/commands.rs src-tauri/src/main.rs src-tauri/src/error.rs
git commit -m "feat(autostart): reconcile OS autostart entry on settings save"
```

---

## Task 6: Boot-time CLI parsing and window-mode application

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Flip the default window visibility to hidden**

In `src-tauri/tauri.conf.json`, the `app.windows[0]` object (lines 13–24) is currently:

```json
{
  "title": "FastClaude",
  "width": 800,
  "height": 600,
  "minWidth": 520,
  "minHeight": 360,
  "decorations": false,
  "resizable": true,
  "shadow": true,
  "transparent": false
}
```

Add `"visible": false` so the autostart Hidden path can leave it that way:

```json
{
  "title": "FastClaude",
  "width": 800,
  "height": 600,
  "minWidth": 520,
  "minHeight": 360,
  "decorations": false,
  "resizable": true,
  "shadow": true,
  "transparent": false,
  "visible": false
}
```

- [ ] **Step 2: Apply launch mode inside `.setup()`**

In `src-tauri/src/main.rs`, update the `use` line at the top to also import the new module:

```rust
use fastclaude_lib::{
    commands::{self, AppState},
    config, launch_args, poller,
    autostart,
    session_registry::Registry,
    spawner, window_focus,
};
```

Then, **immediately after** the existing hotkey-registration block (right after the `match hotkey_str.parse::<...>` block ends near line 64 — find the closing brace of the `match` and the comment for the poller spawn block that follows), insert this:

```rust
            // ─── Apply launch mode to the main window ─────────────────────
            // Default window visibility is `false` (see tauri.conf.json). We
            // either keep it hidden (autostart + Hidden mode) or show it now.
            let launch = launch_args::parse_launch_args(std::env::args());
            let mode = cfg_arc.lock().unwrap().launch_mode;
            if let Some(w) = app.get_webview_window("main") {
                use fastclaude_lib::config::LaunchMode;
                match (launch.from_autostart, mode) {
                    (true, LaunchMode::Hidden) => {
                        // leave hidden; only the global hotkey will reveal it
                    }
                    (true, LaunchMode::Minimized) => {
                        let _ = w.show();
                        let _ = w.minimize();
                    }
                    _ => {
                        // Manual launch, or autostart in Window mode → show normally
                        let _ = w.show();
                    }
                }
            }

            // ─── Path-drift warning ────────────────────────────────────────
            // If config says autostart is on but the OS doesn't have us
            // registered, the most likely cause is a reinstall to a new path.
            // Don't auto-repair — just log so the user notices on next Save.
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                if cfg_arc.lock().unwrap().launch_on_login {
                    match app.autolaunch().is_enabled() {
                        Ok(false) => eprintln!(
                            "autostart: config says enabled but OS registry says disabled (path drift?)"
                        ),
                        Ok(true) => {}
                        Err(e) => eprintln!("autostart: is_enabled() check failed: {e}"),
                    }
                }
            }
```

- [ ] **Step 3: Verify everything compiles**

```
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: clean compile.

- [ ] **Step 4: Verify dev-mode behavior interactively**

Run the app in dev:

```
npm run tauri dev
```

Expected behavior:
- App window appears normally (`from_autostart` is false on manual launch → falls through to `_ => w.show()`).
- No registry writes (`reconcile_autostart` is no-op in debug builds).
- Open Settings → check that future-task UI placeholder isn't yet visible (it lands in Task 7).

Close the app when verified.

- [ ] **Step 5: Run all tests**

```
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```
git add src-tauri/tauri.conf.json src-tauri/src/main.rs
git commit -m "feat(autostart): apply launch mode to main window on boot"
```

---

## Task 7: Add the `Startup` section to Settings UI

**Files:**
- Modify: `src/components/Settings.tsx`

- [ ] **Step 1: Add the new section between Hotkey and Theme**

In `src/components/Settings.tsx`, locate the closing `</Section>` of the `Hotkey` section (currently around line 213, right before `<Section title="Theme">` at line 215).

Immediately after `</Section>` and before the `<Section title="Theme">`, insert:

```tsx
        <Section title="Startup">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm">Launch on login</div>
              <div className="text-xs text-muted-foreground">
                Start FastClaude automatically when you sign in to Windows.
              </div>
            </div>
            <input
              type="checkbox"
              className="h-4 w-4 accent-accent"
              checked={draft.launch_on_login}
              onChange={(e) =>
                setDraft({ ...draft, launch_on_login: e.target.checked })
              }
            />
          </div>
          <Field label="Launch as">
            <Select
              value={draft.launch_mode}
              onValueChange={(v) =>
                setDraft({ ...draft, launch_mode: v as AppConfig["launch_mode"] })
              }
              disabled={!draft.launch_on_login}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="window">Window</SelectItem>
                <SelectItem value="minimized">Minimized</SelectItem>
                <SelectItem value="hidden">Hidden (hotkey only)</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          <p className="text-xs text-muted-foreground">
            'Hidden' relies on your global hotkey — set one in the Hotkey section first.
          </p>
        </Section>
```

- [ ] **Step 2: Warn at save time if Hidden + no hotkey**

In `src/components/Settings.tsx`, find the existing `save()` function (currently around lines 96–106). Replace it with:

```tsx
  async function save() {
    if (!draft) return;
    try {
      await setConfig(draft);
      toast({ title: "Settings saved" });
      if (
        draft.launch_on_login &&
        draft.launch_mode === "hidden" &&
        !draft.hotkey.trim()
      ) {
        toast({
          title: "Hidden mode set with no hotkey",
          description:
            "You'll only be able to reach the window via Task Manager. Configure a hotkey in the Hotkey section.",
          variant: "destructive",
        });
      }
      onBack();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Failed to save", description: msg, variant: "destructive" });
    }
  }
```

- [ ] **Step 3: Verify TypeScript compiles**

```
npm run build
```

Expected: `tsc` step passes; vite build completes.

- [ ] **Step 4: Verify the UI renders correctly**

```
npm run tauri dev
```

Open Settings, scroll to the new "Startup" section. Confirm:
- Toggle reflects `launch_on_login` (off by default for fresh installs).
- "Launch as" select is grayed out when the toggle is off.
- Selecting `Hidden` shows the hint about the hotkey.
- Clicking Save returns to the dashboard with a "Settings saved" toast.

Close the dev app when verified.

- [ ] **Step 5: Commit**

```
git add src/components/Settings.tsx
git commit -m "feat(ui): add Startup section to Settings"
```

---

## Task 8: Manual end-to-end verification

**Files:** none (verification only).

This task validates the registry integration, which can't be unit tested. Execute the checklist below in a release build.

- [ ] **Step 1: Build the release binary and install it**

```
npm run tauri build
```

Locate the installer under `src-tauri/target/release/bundle/` (NSIS `.exe` or MSI). Install it. (If you already have FastClaude installed at a stable path, you can also point this at the existing exe by running it directly from `src-tauri/target/release/fastclaude.exe`, but the registry entry's path will reflect whichever exe you used.)

- [ ] **Step 2: Verify the toggle off → no registry entry**

Launch the installed FastClaude. Open Settings. Confirm `Launch on login` is off. Click Save (no change). Run from a PowerShell window:

```
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v FastClaude
```

Expected: `ERROR: The system was unable to find the specified registry key or value.`

- [ ] **Step 3: Verify enabling writes the registry entry**

Toggle `Launch on login` ON, leave `Launch as` as `Window`. Click Save. Run the same `reg query`:

```
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v FastClaude
```

Expected: a value pointing at the FastClaude exe with `--launched-by-autostart` in the args. (The plugin uses the product name as the key; if `FastClaude` doesn't show up, list all entries: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"`.)

- [ ] **Step 4: Reboot and confirm `Window` mode**

Reboot Windows. After login, FastClaude's main window should appear normally on the desktop.

- [ ] **Step 5: Confirm `Minimized` mode**

Open Settings, change `Launch as` to `Minimized`, Save. Reboot. After login, FastClaude should be present as a minimized taskbar entry — not popped open.

- [ ] **Step 6: Confirm `Hidden` mode**

Open Settings, change `Launch as` to `Hidden`. Make sure a global hotkey is configured (default `Ctrl+Shift+C`). Save. Reboot. After login, no FastClaude window should appear. Press the global hotkey — the window should appear.

- [ ] **Step 7: Confirm toggling off removes the registry entry**

Open Settings, toggle `Launch on login` OFF. Save. Re-run:

```
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v FastClaude
```

Expected: key/value gone.

- [ ] **Step 8: Confirm dev-mode is a no-op**

Run `npm run tauri dev`. Toggle `Launch on login` ON in Settings, Save. Re-run the `reg query`. Expected: **no** entry was written. (Dev mode short-circuits `reconcile_autostart`.) Toggle OFF and Save to clean up the in-memory state.

---

## Out-of-scope follow-ups

These intentionally aren't in this plan but might come up in review:

- **macOS / Linux verification.** The plugin handles both, but neither is on the verification path here.
- **System tray icon.** Mentioned in the spec as a future-want; not blocking Hidden mode for hotkey users.
- **Visual indicator in the Dashboard when autostart is on.** Settings is the source of truth; no extra surface for now.
