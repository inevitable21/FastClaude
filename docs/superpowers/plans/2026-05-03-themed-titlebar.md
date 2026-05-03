# Themed Title Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the OS title bar with a themed bar that absorbs the existing Dashboard top row, so every view shows exactly one warm-aurora-themed bar with view-aware title and window controls.

**Architecture:** A single React `<TitleBar />` is rendered globally in `App.tsx` and receives `view` plus a per-view `rightActions` slot. The OS chrome is hidden via `decorations: false`. Window control behavior uses the built-in Tauri `getCurrentWindow()` JS API — no new Rust code.

**Tech Stack:** React 19, Tailwind CSS, lucide-react icons, Tauri 2 (`@tauri-apps/api/window`).

**Source spec:** `docs/superpowers/specs/2026-05-03-themed-titlebar-design.md`.

**Sequencing strategy:** Build the themed bar fully, wire it in *while OS chrome is still present* (so we never have a barless window), strip per-view headers one at a time, then turn off OS decorations as the final flip. Each task ends with a commit so the build is always shippable.

**Testing approach:** This is window chrome — no automated tests (the project has no test runner configured, adding one is out of scope). Each task includes manual verification steps. The full acceptance checklist runs at the end as Task 8.

---

## File structure

| File | Action | Responsibility |
|---|---|---|
| `src/components/TitleBar.tsx` | Create | Main title bar component, including inline `WindowControls` and `BackButton` exports. |
| `src/components/DashboardActions.tsx` | Create | The Launch / History / Settings cluster, extracted from the current Dashboard header. |
| `src/App.tsx` | Modify | Render `<TitleBar />` once at top; pass per-view `rightActions`. |
| `src/components/Dashboard.tsx` | Modify | Strip the entire existing header `<div>` (current lines 52–83). |
| `src/components/Settings.tsx` | Modify | Strip the existing back-button row (current lines 115–125). |
| `src/components/History.tsx` | Modify | Strip the existing back-button row (current lines 176–198). Move the "Clear all" button into the content area as a small inline toolbar above the session list. |
| `src-tauri/tauri.conf.json` | Modify | Set `decorations: false`, add `minWidth`/`minHeight`, ensure `resizable: true`, `shadow: true`. |
| `src-tauri/capabilities/default.json` | Modify | Add the five `core:window:*` permissions. |

---

### Task 1: Create `TitleBar.tsx`

Build the title bar component with three exports: `TitleBar` (the main bar), `BackButton` (used by Settings/History), and `WindowControls` (used internally only, but defined in the same file for cohesion).

**Files:**
- Create: `src/components/TitleBar.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { ReactNode, useEffect, useState } from "react";
import { ArrowLeft, Minus, Square, X, Copy } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

type View = "dashboard" | "settings" | "history" | "onboarding";

const TITLES: Record<View, string> = {
  dashboard: "FastClaude",
  settings: "Settings",
  history: "History",
  onboarding: "FastClaude",
};

export function BackButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      aria-label="Back"
      title="Back"
      className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
    >
      <ArrowLeft className="h-4 w-4" />
    </button>
  );
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let mounted = true;
    win.isMaximized().then((v) => {
      if (mounted) setMaximized(v);
    });
    const unlistenP = win.onResized(() => {
      win.isMaximized().then((v) => {
        if (mounted) setMaximized(v);
      });
    });
    return () => {
      mounted = false;
      unlistenP.then((un) => un());
    };
  }, []);

  const win = getCurrentWindow();

  const baseBtn =
    "inline-flex h-[30px] w-[30px] items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background";

  return (
    <div className="flex items-center gap-1.5">
      <button
        onClick={() => win.minimize()}
        aria-label="Minimize"
        title="Minimize"
        className={`${baseBtn} hover:bg-[rgba(217,119,87,0.18)] hover:border-[var(--border-strong)]`}
      >
        <Minus className="h-3.5 w-3.5" />
      </button>
      <button
        onClick={() => win.toggleMaximize()}
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
        className={`${baseBtn} hover:bg-[rgba(217,119,87,0.18)] hover:border-[var(--border-strong)]`}
      >
        {maximized ? <Copy className="h-3 w-3" /> : <Square className="h-3 w-3" />}
      </button>
      <button
        onClick={() => win.close()}
        aria-label="Close"
        title="Close"
        className={`${baseBtn} hover:bg-[rgba(180,90,60,0.30)] hover:border-[rgba(248,113,113,0.5)] hover:text-[#FCA5A5]`}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

export function TitleBar({
  view,
  rightActions,
}: {
  view: View;
  rightActions?: ReactNode;
}) {
  return (
    <div className="sticky top-0 z-30 flex items-center gap-2 px-4 py-2 border-b border-border bg-background/55 backdrop-blur-xl h-11">
      <div
        data-tauri-drag-region
        className="flex items-center gap-2.5 font-semibold tracking-tight"
      >
        <img
          src="/icon.png"
          alt=""
          aria-hidden
          data-tauri-drag-region
          className="h-[22px] w-[22px] rounded-md shadow-[0_0_12px_rgba(217,119,87,.4)] flex-shrink-0 pointer-events-none"
        />
        <span data-tauri-drag-region>{TITLES[view]}</span>
      </div>
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      {rightActions}
      <WindowControls />
    </div>
  );
}
```

**Notes on the code:**
- `data-tauri-drag-region` is on the brand block, the title text, the logo, and the flex spacer. That way the bar stays draggable even when the spacer compresses to zero at narrow widths. The logo has `pointer-events-none` so the drag attribute is what wins; otherwise the `<img>` would intercept the drag.
- Maximize-state icon swap uses lucide's `Square` (single square) when not maximized and `Copy` (two overlapping squares — visually correct for "restore") when maximized.
- The cleanup pattern in `WindowControls` accounts for `onResized` returning a `Promise<UnlistenFn>`. We resolve it on unmount.
- Listed sizes (`h-[30px]`, `h-[22px]`) are intentionally arbitrary — they match the spec exactly rather than rounding to Tailwind's nearest unit.

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/components/TitleBar.tsx
git commit -m "feat(ui): add themed TitleBar component with window controls"
```

---

### Task 2: Create `DashboardActions.tsx`

Extract the Launch / History / Settings cluster from the current Dashboard header into a reusable component that the title bar can render in its `rightActions` slot.

**Files:**
- Create: `src/components/DashboardActions.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { Plus, History as HistoryIcon, Settings as SettingsIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

export function DashboardActions({
  onLaunch,
  onOpenHistory,
  onOpenSettings,
}: {
  onLaunch: () => void;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <Button onClick={onLaunch}>
        <Plus className="h-4 w-4" />
        Launch new session
      </Button>
      <button
        onClick={onOpenHistory}
        title="History"
        aria-label="History"
        className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
      >
        <HistoryIcon className="h-4 w-4" />
      </button>
      <button
        onClick={onOpenSettings}
        title="Settings"
        aria-label="Settings"
        className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
      >
        <SettingsIcon className="h-4 w-4" />
      </button>
    </div>
  );
}
```

This is character-identical to the equivalent block currently inside `Dashboard.tsx` (lines 63–82) — same classes, same icons, same Button — just lifted into its own file with three callbacks.

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/components/DashboardActions.tsx
git commit -m "feat(ui): extract DashboardActions into its own component"
```

---

### Task 3: Wire TitleBar into `App.tsx`

Render the title bar globally and pass it the right actions for the current view. The Dashboard, Settings, and History views still have their own headers at this point — the app will visibly show two bars stacked. That's intentional; we strip those headers in tasks 4–6.

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Replace `App.tsx` with the wired version**

Replace the entire contents of `src/App.tsx` with:

```tsx
import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Onboarding } from "@/components/Onboarding";
import { History } from "@/components/History";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired, getFirstRun } from "@/lib/ipc";
import { UpdateBanner } from "@/components/UpdateBanner";
import { AuroraBackground } from "@/components/AuroraBackground";
import { TitleBar, BackButton } from "@/components/TitleBar";
import { DashboardActions } from "@/components/DashboardActions";

type View = "dashboard" | "settings" | "onboarding" | "history";

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [launchOpen, setLaunchOpen] = useState(false);

  useEffect(() => {
    getFirstRun()
      .then((isFirst) => {
        if (isFirst) {
          setView("onboarding");
        } else {
          setView("dashboard");
          setLaunchOpen(true);
        }
      })
      .catch(() => setView("dashboard"));
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onHotkeyFired(() => {
      setView("dashboard");
      setLaunchOpen(true);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  if (view === null) return null;

  const rightActions =
    view === "dashboard" ? (
      <DashboardActions
        onLaunch={() => setLaunchOpen(true)}
        onOpenHistory={() => setView("history")}
        onOpenSettings={() => setView("settings")}
      />
    ) : view === "settings" || view === "history" ? (
      <BackButton onClick={() => setView("dashboard")} />
    ) : null;

  return (
    <>
      <AuroraBackground />
      <div className="min-h-screen flex flex-col text-foreground relative z-10">
        <TitleBar view={view} rightActions={rightActions} />
        {view !== "onboarding" && <UpdateBanner />}
        <div className="flex-1 flex flex-col">
          {view === "onboarding" ? (
            <Onboarding onDone={() => setView("dashboard")} />
          ) : view === "dashboard" ? (
            <Dashboard
              onOpenSettings={() => setView("settings")}
              onOpenHistory={() => setView("history")}
              launchOpen={launchOpen}
              setLaunchOpen={setLaunchOpen}
            />
          ) : view === "history" ? (
            <History onBack={() => setView("dashboard")} />
          ) : (
            <Settings onBack={() => setView("dashboard")} />
          )}
        </div>
        <Toaster />
      </div>
    </>
  );
}
```

The shape of what each child view receives is unchanged from before — `Dashboard` still gets `launchOpen`/`setLaunchOpen` (its `EmptyState` still uses them), `Settings`/`History` still get `onBack`. We are only adding a global title bar above them; the per-view headers will be stripped in tasks 4–6.

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Verify the dev server still launches the app**

Run: `npm run tauri dev`
Expected: app window opens. You will see (intentionally, until tasks 4–6) the OS title bar at top, then the new themed bar with logo + "FastClaude" + Launch/History/Settings + window control buttons, then the Dashboard's *own* header below that. Click the new themed window-control buttons — they should minimize, maximize, and close the window correctly. Close the dev server with `Ctrl+C`.

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): render global TitleBar above all views"
```

---

### Task 4: Strip the Dashboard header

Remove the now-redundant header block from Dashboard so the bar isn't doubled up.

**Files:**
- Modify: `src/components/Dashboard.tsx`

- [ ] **Step 1: Strip imports and the header `<div>`**

In `src/components/Dashboard.tsx`:

Replace:
```tsx
import { useCallback, useEffect } from "react";
import { useState } from "react";
import { Plus, History as HistoryIcon, Settings as SettingsIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { listSessions, onSessionChanged, getConfig } from "@/lib/ipc";
import type { Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";
```

With:
```tsx
import { useCallback, useEffect, useState } from "react";
import { listSessions, onSessionChanged, getConfig } from "@/lib/ipc";
import type { Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";
```

The `Button`, `Plus`, `HistoryIcon`, `SettingsIcon` imports are no longer needed after the header is removed.

Then replace:
```tsx
export function Dashboard({
  onOpenSettings,
  onOpenHistory,
  launchOpen,
  setLaunchOpen,
}: {
  onOpenSettings: () => void;
  onOpenHistory: () => void;
  launchOpen: boolean;
  setLaunchOpen: (v: boolean) => void;
}) {
```

With (the `onOpenSettings` and `onOpenHistory` props are now used only by the title bar in App.tsx, but we keep them in the signature for now — leaving Dashboard's API stable. They'll fall out as dead props in the next step if you want to clean up):
```tsx
export function Dashboard({
  launchOpen,
  setLaunchOpen,
}: {
  launchOpen: boolean;
  setLaunchOpen: (v: boolean) => void;
}) {
```

Then in App.tsx, update the `<Dashboard ... />` call to drop the unused props:
```tsx
<Dashboard
  launchOpen={launchOpen}
  setLaunchOpen={setLaunchOpen}
/>
```

Then in Dashboard.tsx, **delete the entire header block** (currently lines 52–83 — the `<div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">` and everything inside it through its closing `</div>`).

The component body should look like:

```tsx
  return (
    <div className="text-foreground">
      <div className="p-4 min-h-[60vh]">
        {sessions.length === 0 ? (
          <EmptyState onLaunch={() => setLaunchOpen(true)} hotkey={hotkey} />
        ) : (
          <>
            <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground mb-3">
              {sessions.length} running session{sessions.length === 1 ? "" : "s"}
            </div>
            <div className="space-y-2">
              {sessions.map((s, i) => (
                <SessionRow key={s.id} session={s} onChange={refresh} index={i} />
              ))}
            </div>
          </>
        )}
      </div>
      <LaunchDialog open={launchOpen} onOpenChange={setLaunchOpen} onLaunched={refresh} />
    </div>
  );
```

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Verify visually**

Run: `npm run tauri dev`
Expected: Dashboard now shows the themed title bar at the top (with Launch/History/Settings on the right) and the session list directly below. No second header row. Close the dev server.

- [ ] **Step 4: Commit**

```bash
git add src/components/Dashboard.tsx src/App.tsx
git commit -m "refactor(ui): remove Dashboard header (now lives in TitleBar)"
```

---

### Task 5: Strip the Settings header

Remove the back-button row from Settings.

**Files:**
- Modify: `src/components/Settings.tsx`

- [ ] **Step 1: Drop the unused `ArrowLeft` import and the header `<div>`**

In `src/components/Settings.tsx`, change the imports line:

From:
```tsx
import { ArrowLeft, Sun, Moon } from "lucide-react";
```

To:
```tsx
import { Sun, Moon } from "lucide-react";
```

Then **delete the entire header block** (currently lines 115–125 — the `<div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">` and everything inside it through its closing `</div>`).

The return statement should now begin:

```tsx
  return (
    <div className="text-foreground">
      <div className="p-4 space-y-4 max-w-xl min-h-[60vh]">
        <Section title="Terminal">
        …
```

The `onBack` prop stays — it's still called by the Cancel and Save buttons inside the form.

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Verify visually**

Run: `npm run tauri dev`. From the dashboard, click the Settings cog. Expected: title bar reads "Settings", a back-arrow appears in the right actions slot of the bar, the form starts directly below the bar with no second header row. Click the back arrow → return to dashboard. Close the dev server.

- [ ] **Step 4: Commit**

```bash
git add src/components/Settings.tsx
git commit -m "refactor(ui): remove Settings header (now lives in TitleBar)"
```

---

### Task 6: Strip the History header and relocate Clear-all

Same as Settings, but the History header also has a "Clear all" button that needs a new home. Move it into a small inline toolbar at the top of the content area, next to the existing "N ended sessions across M folders" caption.

**Files:**
- Modify: `src/components/History.tsx`

- [ ] **Step 1: Drop the unused import and rework the header**

In `src/components/History.tsx`, change the imports line:

From:
```tsx
import { ArrowLeft, ChevronDown, ChevronRight, Trash2 } from "lucide-react";
```

To:
```tsx
import { ChevronDown, ChevronRight, Trash2 } from "lucide-react";
```

Then **delete the entire header block** (currently lines 176–198 — the `<div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">` through its closing `</div>`).

The return now starts:

```tsx
  return (
    <div className="text-foreground">
      <div className="p-4 min-h-[60vh]">
```

Now relocate the "Clear all" button. Find this block (currently around lines 207–210):

```tsx
            <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground mb-3">
              {totalSessions} ended session{totalSessions === 1 ? "" : "s"} across {groups.length} folder{groups.length === 1 ? "" : "s"}
            </div>
```

Replace it with:

```tsx
            <div className="flex items-center mb-3">
              <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
                {totalSessions} ended session{totalSessions === 1 ? "" : "s"} across {groups.length} folder{groups.length === 1 ? "" : "s"}
              </div>
              <div className="flex-1" />
              {totalSessions > 0 && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setPendingDelete({ kind: "all" })}
                  className="text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  Clear all
                </Button>
              )}
            </div>
```

The `Button` import is already present in this file (line 3).

- [ ] **Step 2: Verify it type-checks**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Verify visually**

Run: `npm run tauri dev`. From the dashboard, click the History clock icon. Expected: title bar reads "History", a back-arrow appears in the right slot, and the "Clear all" button now sits on the right side of the "N ended sessions" caption row inside the content. Test that Clear all still works and that the back arrow returns to dashboard. Close the dev server.

- [ ] **Step 4: Commit**

```bash
git add src/components/History.tsx
git commit -m "refactor(ui): remove History header and inline Clear-all into content"
```

---

### Task 7: Hide OS chrome and grant window-control capabilities

Flip `decorations: false` and grant the five `core:window:*` permissions. This is the moment the OS title bar disappears and the themed bar becomes the only chrome.

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Update `tauri.conf.json`**

Replace the `app.windows` array contents:

From:
```json
"windows": [
  {
    "title": "FastClaude",
    "width": 800,
    "height": 600
  }
],
```

To:
```json
"windows": [
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
],
```

- [ ] **Step 2: Update `capabilities/default.json`**

Replace the contents of `src-tauri/capabilities/default.json`:

From:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "global-shortcut:allow-register",
    "global-shortcut:allow-unregister",
    "global-shortcut:allow-is-registered",
    "updater:default"
  ]
}
```

To:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
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
    "updater:default"
  ]
}
```

- [ ] **Step 3: Run dev to verify the OS chrome is gone**

Run: `npm run tauri dev`
Expected: window opens with only the themed bar at the top — no Windows title bar above it. The window can still be resized by dragging any edge, and the themed window-control buttons still work. Close the dev server.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat(window): hide OS chrome and grant window-control capabilities"
```

---

### Task 8: Manual acceptance test

Walk through the full acceptance checklist from the spec. This task makes no commits — it is a verification gate.

**Files:** none.

- [ ] **Step 1: Launch the dev build**

Run: `npm run tauri dev`

- [ ] **Step 2: Run the spec acceptance checklist**

Verify each of the 12 items from `docs/superpowers/specs/2026-05-03-themed-titlebar-design.md` § "Acceptance checklist":

1. Window opens borderless with themed bar visible across the top.
2. Dragging the empty middle of the bar moves the window.
3. Double-clicking the empty middle maximizes the window; double-click again restores. Maximize icon swaps between `Square` and `Copy` (overlapping squares) both times.
4. Minimize, maximize, and close buttons each perform the correct action.
5. Window can be resized by dragging any edge.
6. `Win+Up`, `Win+Down`, `Win+Left`, `Win+Right` keyboard snaps fire `resize` events; the maximize icon updates correctly when going to/from the maximized state.
7. Light/dark theme toggle (Settings → Theme → Switch) shows correct bar colors, including the close-hover wash.
8. On Settings, the bar reads "Settings", shows a back arrow that returns to Dashboard, and Settings content has no duplicate header.
9. On History, the bar reads "History", the back arrow returns to Dashboard, and the relocated "Clear all" button works.
10. On Onboarding (force this with `getFirstRun() === true` if needed — easiest is to delete the FastClaude config dir and relaunch, or temporarily edit `App.tsx` to set `view === "onboarding"`), the bar reads "FastClaude", has no action slot, and the onboarding flow still works end-to-end.
11. The bar stays at the top while view content scrolls underneath.
12. The OS taskbar label still reads "FastClaude" regardless of which view is open.

- [ ] **Step 3: Resolve any failures**

If any item fails: file a bug, fix it, and re-run the checklist. If the failure is a regression caused by the new bar (not a pre-existing issue), the fix belongs in this branch before merging.

- [ ] **Step 4: Tag the branch as ready for review**

No commit needed if everything passed. The branch is ready for PR; consider running the `superpowers:finishing-a-development-branch` skill.

---

## Self-review

**Spec coverage:** every spec section maps to a task —

| Spec section | Task |
|---|---|
| Architecture / file structure | All tasks |
| `TitleBar` component | Task 1 |
| Tauri configuration | Task 7 |
| `capabilities/default.json` | Task 7 |
| App integration / `App.tsx` | Task 3 |
| Dashboard, Settings, History changes | Tasks 4, 5, 6 |
| Onboarding (unchanged in body) | Task 8 (manual verify only — no code change) |
| Edge cases | Task 8 (manual verify) |
| Acceptance checklist | Task 8 |

**Placeholder scan:** none. All tasks contain actual code or actual JSON, with concrete commands and concrete expected output.

**Type/name consistency:**
- `getCurrentWindow()` is used identically across Task 1 and the spec.
- `View` type matches in Task 1 and Task 3.
- `BackButton`, `WindowControls`, `TitleBar`, `DashboardActions` names are stable across tasks.
- `setLaunchOpen` is plumbed from App.tsx → Dashboard (preserved) and App.tsx → DashboardActions (new) — no rename.
- `onBack` prop on Settings/History stays, called by the same buttons it always was (Cancel/Save in Settings, the now-relocated… wait, History's `onBack` was wired only to the now-deleted back button. Task 6 deletes that wiring; the prop is replaced by the BackButton in the title bar slot, so the prop becomes dead.)

The History `onBack` prop becomes effectively unused after Task 6 — the title bar's `BackButton` handles navigation. Same for Settings: `onBack` is still called by Cancel and Save inside the form, so it stays. **Action:** in Task 6, we can leave the unused `onBack` prop in History to keep the patch surgical, or drop it. Drop is cleaner — the next step adjusts the Task 6 code. Going with: keep the prop for now (it's harmless and one-line) so this plan stays focused. Cleanup is a follow-up.
