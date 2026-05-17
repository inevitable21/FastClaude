# Default Prompt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user save a default prompt in Settings that pre-fills the LaunchDialog's existing "Starting prompt" field on every launch.

**Architecture:** A single new `default_prompt: String` field on `AppConfig` (Rust + TS), surfaced via a new shadcn-style `Textarea` primitive in both Settings and LaunchDialog. No backend command changes — `build_claude_command` already handles `Option<&str>` for prompt.

**Tech Stack:** Tauri 2 / Rust 2021 (serde), React 19, TypeScript ~5.8, Tailwind 3.4.

**Source spec:** `docs/superpowers/specs/2026-05-03-default-prompt-design.md`.

**Sequencing strategy:** Add the new Textarea primitive on its own (standalone, fully green commit). Then ship the entire `default_prompt` contract change in one atomic feature commit — Rust config + Rust unit test + TS type + both UI consumers — so the build never enters a half-wired state where tsc fails. Manual acceptance last.

**Testing approach:** Rust gets one new unit test (round-trip of a config missing `default_prompt`). React UI is verified manually since the project has no JS test runner — adding one is out of scope.

---

## File structure

| File | Action | Responsibility |
|---|---|---|
| `src/components/ui/textarea.tsx` | Create | New shadcn-style `Textarea` primitive (~15 lines). |
| `src-tauri/src/config.rs` | Modify | Add `default_prompt: String` field with `#[serde(default)]`, update `impl Default`, add a missing-field round-trip test. |
| `src/types.ts` | Modify | Add `default_prompt: string` to `AppConfig`. |
| `src/components/Settings.tsx` | Modify | Import `Textarea`. Add a "Default prompt" `<Field>` to the existing "Defaults" `<Section>`. |
| `src/components/LaunchDialog.tsx` | Modify | Import `Textarea`. Seed `prompt` state from `cfg.default_prompt` in the existing config-loading `useEffect`. Replace the `<Input>` prompt field with `<Textarea>`. |

No changes to: `src-tauri/src/commands.rs`, `src-tauri/src/spawner/`, `src/lib/ipc.ts`, `src/lib/launch-options.ts`.

---

### Task 1: Create the `Textarea` primitive

A new shadcn-style component that mirrors `Input`'s look and feel. Standalone, no dependencies on the rest of this feature, so it lands cleanly with `tsc` passing.

**Files:**
- Create: `src/components/ui/textarea.tsx`

- [ ] **Step 1: Write the component**

Create `src/components/ui/textarea.tsx` with:

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {}

const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, ...props }, ref) => (
    <textarea
      ref={ref}
      className={cn(
        "input-fill flex w-full rounded-md border border-border px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 resize-y min-h-[88px]",
        className,
      )}
      {...props}
    />
  ),
);
Textarea.displayName = "Textarea";

export { Textarea };
```

Notes:
- `min-h-[88px]` ≈ 4 rows visible at default font size.
- `resize-y` allows vertical grow only; horizontal resize would break the form layout.
- Reuses the same `input-fill` utility, `border-border`, and focus tokens as `Input`, so it inherits both light and dark theming automatically.
- The `cn` helper at `@/lib/utils` already exists in the project (used by `Input`, `Button`, etc.).

- [ ] **Step 2: Verify TypeScript compiles cleanly**

Run from `C:\GitProjects\FastClaude`:
```
npx tsc --noEmit
```
Expected: zero errors. The new file is standalone — it must not introduce any errors. (No existing file consumes `default_prompt` yet, so the rest of the project still type-checks.)

- [ ] **Step 3: Commit**

```bash
git add src/components/ui/textarea.tsx
git commit -m "feat(ui): add Textarea primitive"
```

---

### Task 2: Wire `default_prompt` through config and both UI consumers

One atomic commit covering: the new Rust `Config` field + a unit test, the matching TS `AppConfig` field, the Settings textarea, and the LaunchDialog textarea + seeding. The build stays green throughout — every step ends in a passing `tsc` and a passing `cargo test`.

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/types.ts`
- Modify: `src/components/Settings.tsx`
- Modify: `src/components/LaunchDialog.tsx`

#### Backend (Rust)

- [ ] **Step 1: Write the failing Rust test**

In `src-tauri/src/config.rs`, inside the existing `mod tests` block (right after the `load_ignores_legacy_pricing_field` test), append:

```rust
    #[test]
    fn load_defaults_prompt_to_empty_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
                "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
        )
        .unwrap();
        let (cfg, _) = load(&path).unwrap();
        assert_eq!(cfg.default_prompt, "");
    }
```

- [ ] **Step 2: Run the test to confirm it fails**

Run from `C:\GitProjects\FastClaude\src-tauri`:
```
cargo test --lib config::tests::load_defaults_prompt_to_empty_when_missing
```
Expected: compile error — `cfg.default_prompt` references a field that doesn't exist on `Config` yet.

- [ ] **Step 3: Add the field to the `Config` struct and its `Default` impl**

In `src-tauri/src/config.rs`, after the existing `default_extra_args` field declaration, add:

```rust
    /// Default prompt sent to claude on launch (empty = don't pass).
    /// LaunchDialog pre-fills its prompt textarea from this value.
    #[serde(default)]
    pub default_prompt: String,
```

So the struct now reads:

```rust
pub struct Config {
    pub terminal_program: String,
    pub default_model: String,
    pub hotkey: String,
    pub idle_threshold_seconds: u64,
    /// Default --effort flag value for new launches; empty string = don't pass.
    /// Valid: "low" | "medium" | "high" | "xhigh" | "max".
    #[serde(default)]
    pub default_effort: String,
    /// Default --permission-mode flag value; empty = don't pass.
    /// Valid: "acceptEdits" | "auto" | "bypassPermissions" | "default" | "dontAsk" | "plan".
    #[serde(default)]
    pub default_permission_mode: String,
    /// Free-form extra args appended verbatim to every launch (unless
    /// overridden in the LaunchDialog).
    #[serde(default)]
    pub default_extra_args: String,
    /// Default prompt sent to claude on launch (empty = don't pass).
    /// LaunchDialog pre-fills its prompt textarea from this value.
    #[serde(default)]
    pub default_prompt: String,
}
```

Then in the `impl Default for Config` block, add `default_prompt: String::new(),` as the last field:

```rust
impl Default for Config {
    fn default() -> Self {
        Self {
            terminal_program: "auto".into(),
            default_model: "claude-opus-4-7".into(),
            hotkey: "Ctrl+Shift+C".into(),
            idle_threshold_seconds: 300,
            default_effort: String::new(),
            default_permission_mode: String::new(),
            default_extra_args: String::new(),
            default_prompt: String::new(),
        }
    }
}
```

- [ ] **Step 4: Run the new test to verify it passes**

Run:
```
cargo test --lib config::tests::load_defaults_prompt_to_empty_when_missing
```
Expected: PASS.

- [ ] **Step 5: Run the full config test module to confirm nothing else regressed**

Run:
```
cargo test --lib config::tests
```
Expected: all six tests pass (the existing five + the new one).

#### Frontend type

- [ ] **Step 6: Add `default_prompt: string` to the `AppConfig` interface**

In `src/types.ts`, add `default_prompt: string` as the last field of `AppConfig`:

```ts
export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
  default_prompt: string;
}
```

`LaunchInput` is unchanged (it already has `prompt?: string`).

- [ ] **Step 7: Run tsc to see the expected downstream errors**

Run from `C:\GitProjects\FastClaude`:
```
npx tsc --noEmit
```
Expected: errors flagged in `Settings.tsx` and/or `LaunchDialog.tsx` because they call `getConfig()` and access fields whose object now requires a `default_prompt` they don't yet handle. **Note exactly which files error** — only those two should be implicated. If errors come from elsewhere, stop and investigate before continuing.

These errors get fixed in the next two sub-tasks. We do NOT commit between here and step 11; the build only goes green again at step 11.

#### Settings UI

- [ ] **Step 8: Add the Textarea import and field in `Settings.tsx`**

At the top of `src/components/Settings.tsx`, after the existing line:
```tsx
import { Input } from "@/components/ui/input";
```
add:
```tsx
import { Textarea } from "@/components/ui/textarea";
```

Then find the existing "Defaults" `<Section>` block. The last `<Field>` inside it is "Default extra args (free-form)" with an `<Input>`. Immediately after that `<Field>`'s closing `</Field>`, and before the `<Section>`'s closing `</Section>`, insert (matching the surrounding 10-space indentation):

```tsx
          <Field label="Default prompt (sent to claude on launch)">
            <Textarea
              value={draft.default_prompt}
              onChange={(e) => setDraft({ ...draft, default_prompt: e.target.value })}
              placeholder="e.g. Review the latest changes and run all tests"
            />
          </Field>
```

#### LaunchDialog UI

- [ ] **Step 9: Add the Textarea import in `LaunchDialog.tsx`**

At the top of `src/components/LaunchDialog.tsx`, find the existing line:
```tsx
import { Input } from "@/components/ui/input";
```
Immediately after it, add:
```tsx
import { Textarea } from "@/components/ui/textarea";
```

- [ ] **Step 10: Seed `prompt` state from `cfg.default_prompt` on dialog open**

Find the existing `useEffect` that loads config (currently around lines 82–96). It currently reads:

```tsx
  useEffect(() => {
    if (!open) return;
    setErr(null);
    setRecentIndex(null);
    recentProjects(10).then(setRecents).catch(() => setRecents([]));
    getConfig()
      .then((c) => {
        setCfg(c);
        setModel(c.default_model);
        setEffort(c.default_effort);
        setPermissionMode(c.default_permission_mode);
        setExtraArgs(c.default_extra_args);
      })
      .catch(() => {});
  }, [open]);
```

Add `setPrompt(c.default_prompt);` as the last line inside the `.then` block:

```tsx
  useEffect(() => {
    if (!open) return;
    setErr(null);
    setRecentIndex(null);
    recentProjects(10).then(setRecents).catch(() => setRecents([]));
    getConfig()
      .then((c) => {
        setCfg(c);
        setModel(c.default_model);
        setEffort(c.default_effort);
        setPermissionMode(c.default_permission_mode);
        setExtraArgs(c.default_extra_args);
        setPrompt(c.default_prompt);
      })
      .catch(() => {});
  }, [open]);
```

The dependency array stays `[open]`. The existing `setPrompt("")` reset inside `submit()` (currently around line 143) STAYS as `""` — after a successful launch the dialog clears the prompt; the next time it opens, the `useEffect` re-fills from config.

- [ ] **Step 11: Replace the prompt `<Input>` with `<Textarea>`**

Find the existing "Starting prompt" block (currently around lines 314–322). It reads:

```tsx
          <div>
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Starting prompt (optional)</label>
            <Input
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Implement X..."
              className="font-sans"
            />
          </div>
```

Replace `Input` with `Textarea`:

```tsx
          <div>
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Starting prompt (optional)</label>
            <Textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Implement X..."
              className="font-sans"
            />
          </div>
```

The only change is `Input` → `Textarea`. Props and structure are identical.

#### Verify and commit

- [ ] **Step 12: Verify TypeScript compiles cleanly**

Run from `C:\GitProjects\FastClaude`:
```
npx tsc --noEmit
```
Expected: zero errors.

If errors remain, read each one carefully. The most common issue would be a typo in `default_prompt` (must be snake_case in the TS type since it's coming through serde from Rust) or a missing closing tag from the JSX edits. Fix in this commit before proceeding — do not split the fix off.

- [ ] **Step 13: Run Rust tests once more to be safe**

Run from `C:\GitProjects\FastClaude\src-tauri`:
```
cargo test --lib config::tests
```
Expected: six tests pass.

- [ ] **Step 14: Commit all four files together**

```bash
git add src-tauri/src/config.rs src/types.ts src/components/Settings.tsx src/components/LaunchDialog.tsx
git commit -m "feat: default prompt setting wired through Settings and LaunchDialog"
```

---

### Task 3: Manual acceptance test

This is UI + config — no further automated tests. Walk through the spec's 8-item checklist. No commits in this task; it's a verification gate.

**Files:** none.

- [ ] **Step 1: Launch the dev build**

Run from `C:\GitProjects\FastClaude`:
```
npm run tauri dev
```

- [ ] **Step 2: Walk the spec acceptance checklist**

Verify each item from `docs/superpowers/specs/2026-05-03-default-prompt-design.md` § "Acceptance checklist":

1. Open Settings → Defaults section now has a "Default prompt" textarea, ~4 rows visible, vertically resizable.
2. Type a default, click Save, close & reopen Settings → value persists.
3. Open LaunchDialog → "Starting prompt (optional)" is now a textarea pre-filled with the default value.
4. Edit the prompt in the dialog and launch → only that launch is affected; Settings still shows the original default.
5. Clear the prompt textarea in the dialog and launch → claude starts with no initial message (verify the live "Will run:" preview shows no positional prompt arg).
6. Set a multi-line default ("Line one.\nLine two.") and launch → claude receives both lines (verify by reading the first user message in claude's UI / its JSONL).
7. Backup the config file, manually delete the `default_prompt` key, restart the app → app starts cleanly, default prompt field shows empty.
8. Light/dark theme: textareas in both Settings and LaunchDialog use the same fill/border/focus tokens as `<Input>` and look correct on both themes.

- [ ] **Step 3: Resolve any failures**

If anything fails, file a bug, fix it in this branch, and re-run the failing item. The Rust unit test from Task 2 covers item 7's basic mechanic but the manual check confirms the full round-trip through the actual Tauri config file.

---

## Self-review

**Spec coverage:**

| Spec section / requirement | Task |
|---|---|
| `Textarea` primitive | Task 1 |
| Backend `Config` field + `Default` impl | Task 2, steps 3 |
| Backend test (missing-field round-trip) | Task 2, steps 1–2, 4–5 |
| Frontend `AppConfig` type | Task 2, step 6 |
| Settings UI field | Task 2, step 8 |
| LaunchDialog seeds prompt from config | Task 2, step 10 |
| LaunchDialog `<Input>` → `<Textarea>` | Task 2, step 11 |
| Edge case: missing field round-trips to empty | Task 2 (Rust test) + Task 3 item 7 (full app round-trip) |
| Edge case: cleared textarea sends no prompt | Task 3, item 5 |
| Edge case: multi-line through shell escape | Task 3, item 6 |
| Edge case: light/dark theming | Task 3, item 8 |

**Placeholder scan:** none. All steps contain actual code, JSON, or commands with concrete expected output.

**Type/name consistency:**
- `default_prompt` (snake_case in Rust + TS) used identically across all references in Task 2.
- `Textarea` import path (`@/components/ui/textarea`) consistent in steps 8 and 9.
- `setPrompt` / `prompt` state names match the existing LaunchDialog code unchanged.

**Build-state invariant:** every commit in this plan ends with `tsc --noEmit` clean and `cargo test --lib config::tests` passing. Task 1 introduces a standalone primitive; Task 2 ships the contract change atomically alongside both consumers, so the build never enters a half-wired state.
