# Futuristic UI (Warm Aurora) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reskin the FastClaude desktop UI with the "Warm Aurora" look — dark by default, frosted-glass panels lit by drifting Claude-orange aurora gradients, Geist typography, subtle motion, selective lucide icons. Light theme exists as a clean cream/coral fallback. No backend, IPC, or session-lifecycle changes.

**Architecture:** Token + targeted component pass. CSS variables in `src/index.css` and the Tailwind config are rewritten so every existing `bg-primary` / `text-foreground` / `border-border` class adopts the new palette automatically. One new component `<AuroraBackground />` mounted in `App.tsx` renders the fixed gradient layer. Each component file gets a className-level restyle; the only file with a behavior change is `Settings.tsx` (gains a Theme toggle). `LaunchDialog.tsx` keyboard logic must stay byte-identical — only its className strings change.

**Tech Stack:** React 19, TypeScript, Vite, Tailwind 3.4, Radix UI, lucide-react (already installed), Tauri 2 (WebView2 / Chromium baseline supports `backdrop-filter` and `color-mix`).

**Spec:** `docs/superpowers/specs/2026-05-01-futuristic-ui-design.md`

**Verification model:** This is UI work in a project with no unit-test harness. Each task's "tests" are (a) `npm run build` (TypeScript + Vite production build — fails on type errors and unresolved imports) and (b) eyeball checks during the final `npm run tauri dev` walkthrough in Task 20. Per-task commits keep blast radius small; if Task 12 breaks the dashboard look, you can revert just that commit.

---

## Task 1: Tailwind config — switch tokens from `hsl(var(...))` to `var(...)` and register Geist

**Files:**
- Modify: `tailwind.config.js`

**Why:** The current config uses `hsl(var(--border))`, expecting HSL channel triplets in CSS. We're switching to HEX/RGBA tokens, so the HSL wrappers must go. Tailwind v3.4 supports plain `var(...)` color references and applies opacity modifiers via `color-mix` automatically. This task only changes the config; the CSS variables themselves are still HSL until Task 2 — that's fine because we're not running between tasks; `npm run build` only validates after both tasks land. To keep this task self-contained and buildable on its own, we *also* land minimal HEX tokens here? No — too coupled. Instead, do this task and Task 2 back-to-back and only run `npm run build` at the end of Task 2.

- [ ] **Step 1: Replace `tailwind.config.js` with the new content**

Path: `tailwind.config.js`

```js
/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    container: { center: true, padding: "2rem" },
    extend: {
      colors: {
        border: "var(--border)",
        "border-strong": "var(--border-strong)",
        input: "var(--input)",
        ring: "var(--ring)",
        background: "var(--background)",
        foreground: "var(--foreground)",
        primary: { DEFAULT: "var(--primary)", foreground: "var(--primary-foreground)" },
        secondary: { DEFAULT: "var(--secondary)", foreground: "var(--secondary-foreground)" },
        destructive: { DEFAULT: "var(--destructive)", foreground: "var(--destructive-foreground)" },
        muted: { DEFAULT: "var(--muted)", foreground: "var(--muted-foreground)" },
        accent: { DEFAULT: "var(--accent)", foreground: "var(--accent-foreground)" },
        card: { DEFAULT: "var(--card)", foreground: "var(--card-foreground)" },
        popover: { DEFAULT: "var(--popover)", foreground: "var(--popover-foreground)" },
        "status-running": "var(--status-running)",
        "status-idle": "var(--status-idle)",
        "status-stopped": "var(--status-stopped)",
      },
      fontFamily: {
        sans: ['Geist', 'ui-sans-serif', 'system-ui', '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Roboto', 'sans-serif'],
        mono: ['"Geist Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', 'monospace'],
      },
      borderRadius: { lg: "var(--radius)", md: "calc(var(--radius) - 2px)", sm: "calc(var(--radius) - 4px)" },
      keyframes: {
        "accordion-down": { from: { height: "0" }, to: { height: "var(--radix-accordion-content-height)" } },
        "accordion-up":   { from: { height: "var(--radix-accordion-content-height)" }, to: { height: "0" } },
        "row-in":         { from: { opacity: "0", transform: "translateY(6px)" }, to: { opacity: "1", transform: "none" } },
        "dot-pulse":      {
          "0%":   { boxShadow: "0 0 0 0 rgba(136,240,192,.55)" },
          "70%":  { boxShadow: "0 0 0 10px rgba(136,240,192,0)" },
          "100%": { boxShadow: "0 0 0 0 rgba(136,240,192,0)" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up":   "accordion-up 0.2s ease-out",
        "row-in":         "row-in 0.42s cubic-bezier(.2,.7,.2,1) both",
        "dot-pulse":      "dot-pulse 2.4s ease-out infinite",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
```

- [ ] **Step 2: Skip build validation — proceed directly to Task 2**

Reason: Task 1 alone leaves the app in a temporarily broken state because the CSS variables are still HSL triplets. Task 2 lands the matching CSS values; we validate after Task 2.

- [ ] **Step 3: Commit**

```bash
git add tailwind.config.js
git commit -m "chore(theme): point Tailwind at var(--token) and register Geist + new keyframes"
```

---

## Task 2: Rewrite `src/index.css` — new tokens, aurora variables, keyframes, reduced-motion block

**Files:**
- Modify: `src/index.css`

- [ ] **Step 1: Replace `src/index.css` with the new content**

Path: `src/index.css`

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  /* Light theme (fallback for bright environments — no aurora, no glass) */
  :root {
    --background: #FDF8F4;
    --foreground: #1A0F0C;
    --muted: #F0E2D6;
    --muted-foreground: #8A766A;
    --popover: #FFFFFF;
    --popover-foreground: #1A0F0C;
    --card: #FFFFFF;
    --card-foreground: #1A0F0C;
    --border: #F0E2D6;
    --border-strong: #E5CFBE;
    --input: #F0E2D6;
    --primary: #C46141;
    --primary-foreground: #FDF8F4;
    --primary-from: #C46141;
    --primary-to: #C46141;
    --secondary: #FAEFE6;
    --secondary-foreground: #1A0F0C;
    --accent: #D97757;
    --accent-foreground: #FDF8F4;
    --destructive: #B45A3C;
    --destructive-foreground: #FDF8F4;
    --ring: #D97757;
    --radius: 0.5rem;

    --aurora-1: rgba(217, 119, 87, 0);
    --aurora-2: rgba(180,  90, 60, 0);
    --aurora-3: rgba(244, 181, 138, 0);

    --status-running: #2F9E68;
    --status-idle:    #B7791F;
    --status-stopped: #6B7280;
  }

  /* Dark theme — Warm Aurora */
  .dark {
    --background: #0D0807;
    --foreground: #FAEFE6;
    --muted: #1A0F0C;
    --muted-foreground: #C9B8AB;
    --popover: #1A0F0C;
    --popover-foreground: #FAEFE6;
    --card: #1A0F0C;
    --card-foreground: #FAEFE6;
    --border: rgba(217, 119, 87, 0.18);
    --border-strong: rgba(217, 119, 87, 0.32);
    --input: rgba(255, 200, 170, 0.12);
    --primary: #C46141;
    --primary-foreground: #1A0F0C;
    --primary-from: #E8825E;
    --primary-to: #C46141;
    --secondary: #1A0F0C;
    --secondary-foreground: #FAEFE6;
    --accent: #F4B58A;
    --accent-foreground: #1A0F0C;
    --destructive: #F87171;
    --destructive-foreground: #1A0F0C;
    --ring: #F4B58A;

    --aurora-1: rgba(217, 119, 87, 0.40);
    --aurora-2: rgba(180,  90, 60, 0.32);
    --aurora-3: rgba(244, 181, 138, 0.18);

    --status-running: #88F0C0;
    --status-idle:    #FBBF24;
    --status-stopped: #6B7280;
  }

  * { @apply border-border; }
  html, body { @apply bg-background text-foreground; }
  body { font-family: theme('fontFamily.sans'); font-feature-settings: "ss01", "cv11"; }
}

/* Aurora layer — used by <AuroraBackground />.
   Animated via gradient-position transform; @property declared so Firefox falls back gracefully. */
@property --ax { syntax: '<percentage>'; inherits: false; initial-value: 12%; }
@property --ay { syntax: '<percentage>'; inherits: false; initial-value: 8%; }
@property --bx { syntax: '<percentage>'; inherits: false; initial-value: 88%; }
@property --by { syntax: '<percentage>'; inherits: false; initial-value: 92%; }
@property --cx { syntax: '<percentage>'; inherits: false; initial-value: 70%; }
@property --cy { syntax: '<percentage>'; inherits: false; initial-value: 22%; }

.aurora-layer {
  position: fixed;
  inset: 0;
  z-index: -1;
  pointer-events: none;
  background:
    radial-gradient(45% 60% at var(--ax) var(--ay), var(--aurora-1), transparent 60%),
    radial-gradient(45% 60% at var(--bx) var(--by), var(--aurora-2), transparent 60%),
    radial-gradient(35% 45% at var(--cx) var(--cy), var(--aurora-3), transparent 65%);
  filter: blur(2px);
  animation: aurora-drift 32s ease-in-out infinite alternate;
}

@keyframes aurora-drift {
  0%   { --ax: 12%; --ay:  8%; --bx: 88%; --by: 92%; --cx: 70%; --cy: 22%; }
  50%  { --ax: 28%; --ay: 22%; --bx: 72%; --by: 78%; --cx: 60%; --cy: 38%; }
  100% { --ax: 18%; --ay: 14%; --bx: 92%; --by: 86%; --cx: 80%; --cy: 18%; }
}

/* Status dot pulse uses the running color directly so it reads on both themes */
.dot-running-glow {
  animation: dot-pulse 2.4s ease-out infinite;
}

@keyframes dot-pulse {
  0%   { box-shadow: 0 0 0 0 var(--status-running); }
  70%  { box-shadow: 0 0 0 10px transparent; }
  100% { box-shadow: 0 0 0 0 transparent; }
}

/* Glass utility used by panels (rows, dialog, toast, banner, settings sections) */
.glass-panel {
  background: color-mix(in oklab, var(--foreground) 4%, transparent);
  border: 1px solid var(--border);
  backdrop-filter: blur(10px);
}

.glass-panel-strong {
  background: color-mix(in oklab, var(--card) 85%, transparent);
  border: 1px solid var(--border-strong);
  backdrop-filter: blur(20px) saturate(140%);
}

/* In light theme, glass panels collapse to solid panels (no blur) */
:root:not(.dark) .glass-panel,
:root:not(.dark) .glass-panel-strong {
  background: var(--card);
  backdrop-filter: none;
}

/* Reduced motion */
@media (prefers-reduced-motion: reduce) {
  .aurora-layer,
  .dot-running-glow,
  .animate-row-in {
    animation: none !important;
  }
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: build succeeds (TypeScript pass + Vite pass). The output bundles will contain `var(--background)` / `color-mix` references but not produce CSS errors.

If build fails because `npm` isn't on PATH in the worktree, run from the repo root with the same path the dev uses.

- [ ] **Step 3: Commit**

```bash
git add src/index.css
git commit -m "feat(theme): rewrite tokens for Warm Aurora dark + clean light fallback"
```

---

## Task 3: `index.html` — preconnect Geist, default to dark class, set proper title

**Files:**
- Modify: `index.html`

- [ ] **Step 1: Replace `index.html` with the new content**

Path: `index.html`

```html
<!doctype html>
<html lang="en" class="dark">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/png" href="/icon.png" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>FastClaude</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link
      rel="stylesheet"
      href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700&family=Geist+Mono:wght@400;500&display=swap"
    />
  </head>

  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add index.html
git commit -m "chore(html): default html to dark, load Geist fonts, set proper title"
```

---

## Task 4: `src/main.tsx` — synchronous theme bootstrap (read localStorage before React renders)

**Files:**
- Modify: `src/main.tsx`

- [ ] **Step 1: Replace `src/main.tsx` with the new content**

Path: `src/main.tsx`

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Apply persisted theme before React mounts so there is no flash.
// `index.html` ships with class="dark"; only flip if the user previously chose light.
const stored = localStorage.getItem("fastclaude-theme");
if (stored === "light") {
  document.documentElement.classList.remove("dark");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/main.tsx
git commit -m "feat(theme): synchronously apply persisted theme before React mounts"
```

---

## Task 5: Create `src/components/AuroraBackground.tsx`

**Files:**
- Create: `src/components/AuroraBackground.tsx`

- [ ] **Step 1: Create the file**

Path: `src/components/AuroraBackground.tsx`

```tsx
// Fixed background layer for the Warm Aurora theme.
// All animation and gradient values live in src/index.css under .aurora-layer
// so this component is just a stable mount point.
export function AuroraBackground() {
  return <div aria-hidden="true" className="aurora-layer" />;
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/AuroraBackground.tsx
git commit -m "feat(ui): add AuroraBackground component"
```

---

## Task 6: `src/App.tsx` — mount `<AuroraBackground />` at the top

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Edit `src/App.tsx` — add import and mount**

Replace the `import` block at the top (lines 1-8) with:

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
```

Then replace the JSX return block (currently `return ( <div className="min-h-screen ..."> ... </div> )`) with:

```tsx
  return (
    <>
      <AuroraBackground />
      <div className="min-h-screen flex flex-col bg-background text-foreground relative">
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
```

The wrapper change is important: `<AuroraBackground />` must be a sibling of the main `<div>`, not a child, because the aurora layer is `position: fixed`. The new outer `<>` fragment plus `relative` on the inner div ensures the page stacks above the aurora.

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): mount AuroraBackground at root"
```

---

## Task 7: `src/components/ui/button.tsx` — variants restyle (primary gradient, frosted ghost, coral-outlined destructive)

**Files:**
- Modify: `src/components/ui/button.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/ui/button.tsx`

```tsx
import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          // Primary: warm-coral gradient with soft shadow + inner highlight.
          // Gradient values come from --primary-from / --primary-to so the
          // light theme automatically collapses to a solid color.
          "text-primary-foreground font-semibold shadow-[0_8px_24px_rgba(217,119,87,.32),inset_0_1px_0_rgba(255,255,255,.18)] hover:brightness-110 [background:linear-gradient(180deg,var(--primary-from),var(--primary-to))]",
        destructive:
          // Coral-outlined danger button (used by Kill / destructive actions).
          "border border-destructive/40 bg-transparent text-destructive hover:bg-destructive/10",
        outline:
          "border border-border bg-transparent text-foreground hover:bg-foreground/5",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        // Ghost: frosted translucent panel; reads on both aurora and solid panels.
        ghost:
          "border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08]",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-11 rounded-md px-8",
        icon: "h-9 w-9 rounded-md p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button"
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    )
  }
)
Button.displayName = "Button"

export { Button, buttonVariants }
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/ui/button.tsx
git commit -m "feat(ui): restyle Button variants for Warm Aurora"
```

---

## Task 8: `src/components/ui/dialog.tsx` — deep glass content, blurred overlay

**Files:**
- Modify: `src/components/ui/dialog.tsx`

- [ ] **Step 1: Edit the `DialogOverlay` className**

Find this block (lines ~18-26):

```tsx
<DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      "fixed inset-0 z-50 bg-black/80 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
      className
    )}
    {...props}
  />
```

Replace the className string with:

```
"fixed inset-0 z-50 bg-black/50 backdrop-blur-sm data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0"
```

- [ ] **Step 2: Edit the `DialogContent` className**

Find this block (lines ~36-43):

```tsx
<DialogPrimitive.Content
      ref={ref}
      className={cn(
        "fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%] sm:rounded-lg",
        className
      )}
```

Replace the className string with:

```
"fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 glass-panel-strong rounded-xl p-6 shadow-2xl duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]"
```

(Removed `border bg-background sm:rounded-lg`, replaced with `glass-panel-strong rounded-xl shadow-2xl`.)

- [ ] **Step 3: Edit the close button (`DialogPrimitive.Close`)**

Find (line ~45):

```tsx
<DialogPrimitive.Close className="absolute right-4 top-4 rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none data-[state=open]:bg-accent data-[state=open]:text-muted-foreground">
```

Replace the className with:

```
"absolute right-4 top-4 rounded-md p-1 text-muted-foreground opacity-70 transition-opacity hover:opacity-100 hover:text-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none"
```

- [ ] **Step 4: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/dialog.tsx
git commit -m "feat(ui): glass dialog panel with blurred overlay"
```

---

## Task 9: `src/components/ui/input.tsx` — mono font, dark translucent bg, accent focus ring

**Files:**
- Modify: `src/components/ui/input.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/ui/input.tsx`

```tsx
import * as React from "react"

import { cn } from "@/lib/utils"

const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<"input">>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-10 w-full rounded-md border border-input bg-foreground/[0.03] px-3 py-2 font-mono text-sm text-foreground ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50",
          className
        )}
        ref={ref}
        {...props}
      />
    )
  }
)
Input.displayName = "Input"

export { Input }
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/ui/input.tsx
git commit -m "feat(ui): mono input with frosted bg and accent focus ring"
```

---

## Task 10: `src/components/ui/select.tsx` — trigger matches input, content gets glass panel

**Files:**
- Modify: `src/components/ui/select.tsx`

- [ ] **Step 1: Edit the `SelectTrigger` className**

Find this className string (line ~20):

```
"flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background data-[placeholder]:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 [&>span]:line-clamp-1"
```

Replace with:

```
"flex h-10 w-full items-center justify-between rounded-md border border-input bg-foreground/[0.03] px-3 py-2 text-sm ring-offset-background data-[placeholder]:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 [&>span]:line-clamp-1"
```

(Only `bg-background` → `bg-foreground/[0.03]`.)

- [ ] **Step 2: Edit the `SelectContent` className**

Find this className string (line ~75):

```
"relative z-50 max-h-[--radix-select-content-available-height] min-w-[8rem] overflow-y-auto overflow-x-hidden rounded-md border bg-popover text-popover-foreground shadow-md data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 origin-[--radix-select-content-transform-origin]"
```

Replace with:

```
"relative z-50 max-h-[--radix-select-content-available-height] min-w-[8rem] overflow-y-auto overflow-x-hidden rounded-md glass-panel-strong text-popover-foreground shadow-2xl data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 origin-[--radix-select-content-transform-origin]"
```

(Replaced `border bg-popover ... shadow-md` with `glass-panel-strong ... shadow-2xl`.)

- [ ] **Step 3: Edit the `SelectItem` className**

Find this className string (line ~118):

```
"relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50"
```

Replace with:

```
"relative flex w-full cursor-default select-none items-center rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none focus:bg-foreground/[0.06] focus:text-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50"
```

(Focus uses subtle frosted highlight rather than the harsh accent fill, which would clash with the glass panel.)

- [ ] **Step 4: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/select.tsx
git commit -m "feat(ui): glass Select content + matching frosted trigger"
```

---

## Task 11: `src/components/ui/toast.tsx` — glass toasts, positioned bottom-right

**Files:**
- Modify: `src/components/ui/toast.tsx`

- [ ] **Step 1: Edit the `toastVariants` className strings**

Find this block (lines ~27-41):

```tsx
const toastVariants = cva(
  "group pointer-events-auto relative flex w-full items-center justify-between space-x-4 overflow-hidden rounded-md border p-6 pr-8 shadow-lg transition-all data-[swipe=cancel]:translate-x-0 data-[swipe=end]:translate-x-[var(--radix-toast-swipe-end-x)] data-[swipe=move]:translate-x-[var(--radix-toast-swipe-move-x)] data-[swipe=move]:transition-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[swipe=end]:animate-out data-[state=closed]:fade-out-80 data-[state=closed]:slide-out-to-right-full data-[state=open]:slide-in-from-top-full data-[state=open]:sm:slide-in-from-bottom-full",
  {
    variants: {
      variant: {
        default: "border bg-background text-foreground",
        destructive:
          "destructive group border-destructive bg-destructive text-destructive-foreground",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)
```

Replace with:

```tsx
const toastVariants = cva(
  "group pointer-events-auto relative flex w-full items-center justify-between gap-3 overflow-hidden rounded-xl p-4 pr-8 shadow-2xl transition-all data-[swipe=cancel]:translate-x-0 data-[swipe=end]:translate-x-[var(--radix-toast-swipe-end-x)] data-[swipe=move]:translate-x-[var(--radix-toast-swipe-move-x)] data-[swipe=move]:transition-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[swipe=end]:animate-out data-[state=closed]:fade-out-80 data-[state=closed]:slide-out-to-right-full data-[state=open]:slide-in-from-top-full data-[state=open]:sm:slide-in-from-bottom-full",
  {
    variants: {
      variant: {
        default: "glass-panel-strong text-foreground",
        destructive:
          "destructive group border border-destructive/40 bg-foreground/[0.04] text-destructive backdrop-blur-md",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/ui/toast.tsx
git commit -m "feat(ui): glass toasts"
```

---

## Task 12: `src/components/Dashboard.tsx` — topbar restyle, brand mark, icon-only buttons

**Files:**
- Modify: `src/components/Dashboard.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/Dashboard.tsx`

```tsx
import { useCallback, useEffect } from "react";
import { useState } from "react";
import { Plus, History as HistoryIcon, Settings as SettingsIcon } from "lucide-react";
import { listSessions, onSessionChanged, getConfig } from "@/lib/ipc";
import type { Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";

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
  const [sessions, setSessions] = useState<Session[]>([]);
  const [hotkey, setHotkey] = useState<string>("");

  useEffect(() => {
    getConfig()
      .then((c) => setHotkey(c.hotkey))
      .catch(() => setHotkey(""));
  }, []);

  const refresh = useCallback(() => {
    listSessions()
      .then(setSessions)
      .catch(() => setSessions([]));
  }, []);

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | null = null;
    onSessionChanged(refresh).then((fn) => {
      unlisten = fn;
    });
    const t = setInterval(refresh, 5000);
    return () => {
      unlisten?.();
      clearInterval(t);
    };
  }, [refresh]);

  return (
    <div className="text-foreground">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">
        <div className="flex items-center gap-2.5 font-semibold tracking-tight">
          <div
            aria-hidden
            className="h-[22px] w-[22px] rounded-md shadow-[0_0_12px_rgba(217,119,87,.4)]"
            style={{ background: "linear-gradient(135deg,#E8825E,#C46141)" }}
          />
          FastClaude
        </div>
        <div className="flex-1" />
        <button
          onClick={() => setLaunchOpen(true)}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm font-semibold text-primary-foreground shadow-[0_8px_24px_rgba(217,119,87,.32),inset_0_1px_0_rgba(255,255,255,.18)] hover:brightness-110 transition"
          style={{ background: "linear-gradient(180deg,var(--primary-from),var(--primary-to))" }}
        >
          <Plus className="h-4 w-4" />
          Launch new session
        </button>
        <button
          onClick={onOpenHistory}
          title="History"
          aria-label="History"
          className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
        >
          <HistoryIcon className="h-4 w-4" />
        </button>
        <button
          onClick={onOpenSettings}
          title="Settings"
          aria-label="Settings"
          className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
        >
          <SettingsIcon className="h-4 w-4" />
        </button>
      </div>
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
}
```

Notes:
- The raster `<img src="/icon.png">` is replaced by the inline gradient `<div>` brand mark.
- `index` is passed to `SessionRow` so it can stagger its mount animation (Task 13).
- The Launch button uses inline `style` for the gradient because Tailwind doesn't synthesize `linear-gradient(...)` from token names cleanly. The same gradient is exposed via the `default` Button variant — but `Dashboard.tsx` uses a raw `<button>` (not the `Button` component) so the visual change here mirrors what the Button variant does.

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/Dashboard.tsx
git commit -m "feat(ui): restyle Dashboard topbar with gradient brand mark and icon buttons"
```

---

## Task 13: `src/components/SessionRow.tsx` — frosted panel, status dot states, coral-outlined Kill

**Files:**
- Modify: `src/components/SessionRow.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/SessionRow.tsx`

```tsx
import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import type { Session } from "@/types";
import { focusSession, killSession } from "@/lib/ipc";

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function elapsed(startedAt: number): string {
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}

function errMessage(e: unknown): string {
  if (typeof e === "string") return e;
  return (e as { message?: string })?.message ?? String(e);
}

export function SessionRow({
  session,
  onChange,
  index = 0,
}: {
  session: Session;
  onChange: () => void;
  index?: number;
}) {
  const { toast } = useToast();

  // Map status to a visual variant. Pulse only on running.
  const dotClass =
    session.status === "running"
      ? "bg-[var(--status-running)] dot-running-glow"
      : session.status === "idle"
      ? "bg-[var(--status-idle)]"
      : "bg-[var(--status-stopped)]";

  const projectName =
    session.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? session.project_dir;

  async function focus() {
    try {
      await focusSession(session.id);
    } catch (e: unknown) {
      toast({
        title: "Couldn't focus session",
        description: errMessage(e),
        variant: "destructive",
      });
    }
    onChange();
  }
  async function kill() {
    try {
      await killSession(session.id);
    } catch (e: unknown) {
      toast({
        title: "Couldn't kill session",
        description: errMessage(e),
        variant: "destructive",
      });
    }
    onChange();
  }

  return (
    <div
      className="flex items-center gap-3 rounded-lg glass-panel p-3 transition-colors hover:border-border-strong animate-row-in"
      style={{ animationDelay: `${index * 70}ms` }}
    >
      <div className={`h-2 w-2 rounded-full flex-shrink-0 ${dotClass}`} />
      <div className="flex-1 min-w-0">
        <div className="font-semibold text-sm truncate">{projectName}</div>
        <div className="text-xs text-muted-foreground truncate font-mono">{session.project_dir}</div>
      </div>
      {session.tokens_out > 0 && (
        <div className="text-xs text-muted-foreground font-mono">
          tokens: {fmtTokens(session.tokens_out)}
        </div>
      )}
      <div className="text-xs text-muted-foreground font-mono">{elapsed(session.started_at)}</div>
      <span className="text-[10px] font-mono px-2 py-0.5 rounded-full border border-accent/35 text-accent bg-accent/10">
        {session.model}
      </span>
      <Button size="sm" variant="ghost" onClick={focus}>
        Focus
      </Button>
      <Button size="sm" variant="destructive" onClick={kill}>
        Kill
      </Button>
    </div>
  );
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success. (Type-checks the new optional `index` prop.)

- [ ] **Step 3: Commit**

```bash
git add src/components/SessionRow.tsx
git commit -m "feat(ui): glass SessionRow with pulsing running dot and accent badge"
```

---

## Task 14: `src/components/EmptyState.tsx` — circular accent icon, kbd chip

**Files:**
- Modify: `src/components/EmptyState.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/EmptyState.tsx`

```tsx
import { Plus } from "lucide-react";

export function EmptyState({
  onLaunch,
  hotkey,
}: {
  onLaunch: () => void;
  hotkey?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center gap-3">
      <div className="flex h-14 w-14 items-center justify-center rounded-full border border-border bg-accent/10 text-accent">
        <Plus className="h-6 w-6" />
      </div>
      <h2 className="text-lg font-semibold">No running sessions</h2>
      <p className="text-sm text-muted-foreground">
        Launch one to get started{hotkey ? <>, or hit <kbd className="inline-block rounded-md border border-border bg-foreground/[0.05] px-1.5 py-0.5 font-mono text-[11px]">{hotkey}</kbd> from anywhere.</> : "."}
      </p>
      <button
        onClick={onLaunch}
        className="mt-2 inline-flex items-center gap-1.5 px-4 py-2 rounded-md text-sm font-semibold text-primary-foreground shadow-[0_8px_24px_rgba(217,119,87,.32),inset_0_1px_0_rgba(255,255,255,.18)] hover:brightness-110 transition"
        style={{ background: "linear-gradient(180deg,var(--primary-from),var(--primary-to))" }}
      >
        <Plus className="h-4 w-4" />
        Launch new session
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/EmptyState.tsx
git commit -m "feat(ui): empty state with accent icon and kbd hotkey chip"
```

---

## Task 15: `src/components/UpdateBanner.tsx` — slim coral gradient bar with dismiss

**Files:**
- Modify: `src/components/UpdateBanner.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/UpdateBanner.tsx`

```tsx
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useToast } from "@/hooks/use-toast";
import { checkForUpdate, installUpdate } from "@/lib/ipc";
import type { UpdateInfo } from "@/types";

export function UpdateBanner() {
  const { toast } = useToast();
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => {
      checkForUpdate().then(setUpdate).catch(() => {});
    }, 5000);
    return () => clearTimeout(t);
  }, []);

  if (!update || dismissed) return null;

  async function install() {
    try {
      await installUpdate();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update failed", description: msg, variant: "destructive" });
    }
  }

  return (
    <div
      className="flex items-center gap-3 px-4 py-2 text-sm border-b border-border-strong"
      style={{
        background:
          "linear-gradient(90deg, rgba(217,119,87,.20), rgba(217,119,87,.06))",
      }}
    >
      <div
        aria-hidden
        className="h-2 w-2 rounded-full bg-accent shadow-[0_0_8px_rgba(244,181,138,.6)]"
      />
      <div className="flex-1">FastClaude {update.version} is available.</div>
      <button
        onClick={install}
        className="inline-flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
      >
        Restart &amp; install
      </button>
      <button
        onClick={() => setDismissed(true)}
        title="Dismiss"
        aria-label="Dismiss"
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/UpdateBanner.tsx
git commit -m "feat(ui): coral gradient update banner with dismiss"
```

---

## Task 16: `src/components/LaunchDialog.tsx` — visual restyle ONLY (className strings)

**Critical constraint:** Recent commits 565a660, 0921fda, 8071cb2, 86be486 chained fixes for arrow-key and Enter handling in this file. **Do not edit any of:** the `useEffect` bodies, the `stateRef` mirroring block, `recentRefs`, `inputRef`, the document-level keydown handler, the props on `<DialogContent>` other than `className`, the `onOpenAutoFocus` handler. Only edit className strings inside the JSX.

**Files:**
- Modify: `src/components/LaunchDialog.tsx`

- [ ] **Step 1: Edit the recent-row className**

Find this block (lines ~239-256):

```tsx
{recents.map((r, i) => (
  <button
    key={r.encoded_name}
    ref={(el) => {
      recentRefs.current[i] = el;
    }}
    onClick={() => {
      setProjectDir(r.decoded_path);
      setRecentIndex(i);
    }}
    className={`block w-full text-left px-2 py-1 text-xs hover:bg-accent ${
      recentIndex === i
        ? "bg-primary text-primary-foreground"
        : ""
    }`}
  >
    {r.decoded_path}
  </button>
))}
```

Replace ONLY the `className` template literal with:

```tsx
className={`block w-full text-left px-2 py-1 text-xs font-mono transition-colors hover:bg-foreground/[0.06] ${
  recentIndex === i
    ? "bg-gradient-to-r from-[rgba(217,119,87,.25)] to-[rgba(217,119,87,.10)] text-foreground border-l-2 border-accent pl-[6px]"
    : "border-l-2 border-transparent"
}`}
```

(The `border-l-2 border-transparent` on the inactive state preserves layout so the active state's left border doesn't shift the row.)

- [ ] **Step 2: Edit the recents container className**

Find this line (~237):

```tsx
<div className="mt-2 max-h-40 overflow-auto border rounded">
```

Replace with:

```tsx
<div className="mt-2 max-h-40 overflow-auto rounded-md border border-border bg-black/20">
```

- [ ] **Step 3: Edit the preview-cmd block**

Find this block (~322-326):

```tsx
{preview && (
  <div className="text-[11px] font-mono bg-muted/50 border border-border rounded p-2 break-all">
    <span className="text-muted-foreground">Will run:</span> {preview}
  </div>
)}
```

Replace with:

```tsx
{preview && (
  <div className="text-[11px] font-mono bg-black/30 border border-border rounded-md p-2 break-all">
    <span className="text-accent">Will run:</span> {preview}
  </div>
)}
```

- [ ] **Step 4: Edit the labels and the terminal-meta line for tracking-uppercase consistency**

Find the four `<label className="text-xs font-medium">...</label>` lines and the `cfg && ...` block (~336-340):

```tsx
{cfg && (
  <div className="text-[10px] text-muted-foreground">
    terminal: {cfg.terminal_program}
  </div>
)}
```

Replace each label className `text-xs font-medium` with `text-[10px] uppercase tracking-[0.10em] text-muted-foreground`. Replace the `cfg && ...` block with:

```tsx
{cfg && (
  <div className="text-[10px] font-mono text-muted-foreground">
    terminal: {cfg.terminal_program}
  </div>
)}
```

(For clarity: there are four `<label>` elements wrapping form fields — Project folder, Model, --effort, --permission-mode, Extra args, Starting prompt. Update all `className="text-xs font-medium"` instances.)

- [ ] **Step 5: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 6: Commit**

```bash
git add src/components/LaunchDialog.tsx
git commit -m "feat(ui): restyle LaunchDialog visuals (no logic changes)"
```

---

## Task 17: `src/components/Onboarding.tsx` — glass card, gradient brand mark, primary button

**Files:**
- Modify: `src/components/Onboarding.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/Onboarding.tsx`

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig, clearFirstRun } from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import type { AppConfig } from "@/types";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8 text-muted-foreground">Loading...</div>;

  async function getStarted() {
    if (!draft) return;
    try {
      await setConfig(draft);
      await clearFirstRun();
      onDone();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't save", description: msg, variant: "destructive" });
    }
  }

  function field(label: string, value: string, onChange: (v: string) => void, hint?: string) {
    return (
      <label className="block">
        <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
        {hint && <div className="text-xs text-muted-foreground mt-1.5">{hint}</div>}
      </label>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center text-foreground px-4">
      <div className="max-w-md w-full p-6 space-y-5 glass-panel-strong rounded-xl shadow-2xl">
        <div className="flex items-center gap-2.5">
          <div
            aria-hidden
            className="h-7 w-7 rounded-md shadow-[0_0_14px_rgba(217,119,87,.45)]"
            style={{ background: "linear-gradient(135deg,#E8825E,#C46141)" }}
          />
          <div>
            <div className="text-xl font-bold tracking-tight">Welcome to FastClaude</div>
            <div className="text-sm text-muted-foreground">Three quick choices and you're set.</div>
          </div>
        </div>
        {field(
          "Terminal program",
          draft.terminal_program,
          (v) => setDraft({ ...draft, terminal_program: v }),
          "'auto' picks Windows Terminal if installed, else cmd.exe"
        )}
        <label className="block">
          <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">Default model</div>
          <Select
            value={draft.default_model}
            onValueChange={(v) => setDraft({ ...draft, default_model: v })}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {MODELS.map((m) => (
                <SelectItem key={m} value={m}>{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        {field(
          "Global hotkey",
          draft.hotkey,
          (v) => setDraft({ ...draft, hotkey: v }),
          "Pressed from anywhere to open the launch dialog"
        )}
        <div className="pt-2">
          <Button onClick={getStarted} className="w-full">Get started</Button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/Onboarding.tsx
git commit -m "feat(ui): glass-card Onboarding with gradient brand mark"
```

---

## Task 18: `src/components/Settings.tsx` — labeled glass sections + new Theme toggle

**Files:**
- Modify: `src/components/Settings.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/Settings.tsx`

```tsx
import { useEffect, useState } from "react";
import { ArrowLeft, Sun, Moon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig, checkForUpdate } from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import {
  EFFORT_OPTIONS,
  PERMISSION_MODE_OPTIONS,
  UNSET,
  fromUnset,
  toUnset,
} from "@/lib/launch-options";
import type { AppConfig } from "@/types";

type Theme = "dark" | "light";

function readTheme(): Theme {
  return localStorage.getItem("fastclaude-theme") === "light" ? "light" : "dark";
}

function applyTheme(t: Theme) {
  if (t === "dark") {
    document.documentElement.classList.add("dark");
    localStorage.setItem("fastclaude-theme", "dark");
  } else {
    document.documentElement.classList.remove("dark");
    localStorage.setItem("fastclaude-theme", "light");
  }
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="glass-panel rounded-xl p-4 space-y-3">
      <div className="text-[10px] uppercase tracking-[0.12em] text-accent">{title}</div>
      {children}
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">
        {label}
      </div>
      {children}
    </label>
  );
}

export function Settings({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);
  const [theme, setTheme] = useState<Theme>(() => readTheme());

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8 text-muted-foreground">Loading...</div>;

  async function checkUpdates() {
    try {
      const u = await checkForUpdate();
      if (u) {
        toast({
          title: `FastClaude ${u.version} available`,
          description: "Restart from the banner to install.",
        });
      } else {
        toast({ title: "You're up to date" });
      }
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update check failed", description: msg, variant: "destructive" });
    }
  }

  async function save() {
    if (!draft) return;
    try {
      await setConfig(draft);
      toast({ title: "Settings saved" });
      onBack();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Failed to save", description: msg, variant: "destructive" });
    }
  }

  function toggleTheme() {
    const next: Theme = theme === "dark" ? "light" : "dark";
    applyTheme(next);
    setTheme(next);
  }

  return (
    <div className="text-foreground">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">
        <button
          onClick={onBack}
          aria-label="Back"
          title="Back"
          className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
        >
          <ArrowLeft className="h-4 w-4" />
        </button>
        <div className="font-semibold tracking-tight">Settings</div>
      </div>
      <div className="p-4 space-y-4 max-w-xl min-h-[60vh]">
        <Section title="Terminal">
          <Field label="Terminal program (or 'auto')">
            <Input
              value={draft.terminal_program}
              onChange={(e) => setDraft({ ...draft, terminal_program: e.target.value })}
            />
          </Field>
        </Section>

        <Section title="Defaults">
          <Field label="Default model">
            <Select
              value={draft.default_model}
              onValueChange={(v) => setDraft({ ...draft, default_model: v })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {MODELS.map((m) => (
                  <SelectItem key={m} value={m}>{m}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Default --effort">
            <Select
              value={toUnset(draft.default_effort)}
              onValueChange={(v) => setDraft({ ...draft, default_effort: fromUnset(v) })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={UNSET}>(don't pass)</SelectItem>
                {EFFORT_OPTIONS.map((e) => (
                  <SelectItem key={e} value={e}>{e}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Default --permission-mode">
            <Select
              value={toUnset(draft.default_permission_mode)}
              onValueChange={(v) =>
                setDraft({ ...draft, default_permission_mode: fromUnset(v) })
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={UNSET}>(don't pass)</SelectItem>
                {PERMISSION_MODE_OPTIONS.map((m) => (
                  <SelectItem key={m} value={m}>{m}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Default extra args (free-form)">
            <Input
              value={draft.default_extra_args}
              onChange={(e) => setDraft({ ...draft, default_extra_args: e.target.value })}
              placeholder='e.g. --name "MyAgent" --no-session-persistence'
            />
          </Field>
        </Section>

        <Section title="Hotkey">
          <Field label="Global hotkey">
            <Input
              value={draft.hotkey}
              onChange={(e) => setDraft({ ...draft, hotkey: e.target.value })}
            />
          </Field>
          <Field label="Idle threshold (seconds)">
            <Input
              value={String(draft.idle_threshold_seconds)}
              onChange={(e) => {
                const n = parseInt(e.target.value, 10);
                if (!Number.isNaN(n) && n > 0) {
                  setDraft({ ...draft, idle_threshold_seconds: n });
                }
              }}
            />
          </Field>
          <p className="text-xs text-muted-foreground">
            Hotkey changes take effect after restart.
          </p>
        </Section>

        <Section title="Theme">
          <div className="flex items-center gap-3">
            <div className="flex-1 text-sm text-muted-foreground">
              Currently: <span className="text-foreground font-medium">{theme === "dark" ? "Dark (Warm Aurora)" : "Light"}</span>
            </div>
            <button
              onClick={toggleTheme}
              className="inline-flex items-center gap-2 px-3 py-1.5 rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition text-sm"
            >
              {theme === "dark" ? (
                <>
                  <Sun className="h-4 w-4" /> Switch to light
                </>
              ) : (
                <>
                  <Moon className="h-4 w-4" /> Switch to dark
                </>
              )}
            </button>
          </div>
        </Section>

        <Section title="Updates">
          <Button variant="ghost" onClick={checkUpdates}>Check for updates</Button>
        </Section>

        <div className="pt-2 flex gap-2 justify-end">
          <Button variant="ghost" onClick={onBack}>Cancel</Button>
          <Button onClick={save}>Save</Button>
        </div>
      </div>
    </div>
  );
}
```

Notes:
- New `theme` state, `readTheme` / `applyTheme` helpers.
- All previous fields are preserved with the same data binding.
- Save/Cancel buttons keep prior behavior.

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/Settings.tsx
git commit -m "feat(settings): glass sections + Theme toggle (persists to localStorage)"
```

---

## Task 19: `src/components/History.tsx` — match Dashboard shell, glass rows, accent badge

**Files:**
- Modify: `src/components/History.tsx`

- [ ] **Step 1: Replace the file**

Path: `src/components/History.tsx`

```tsx
import { useCallback, useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import { launchSession, listAllSessions, onSessionChanged } from "@/lib/ipc";
import type { Session } from "@/types";

const HISTORY_LIMIT = 50;

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function relativeTime(epochSecs: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function duration(startedAt: number, endedAt: number | null): string {
  const end = endedAt ?? Math.floor(Date.now() / 1000);
  const secs = Math.max(0, end - startedAt);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m`;
  return `${secs}s`;
}

export function History({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [sessions, setSessions] = useState<Session[] | null>(null);

  const refresh = useCallback(() => {
    listAllSessions()
      .then((all) =>
        setSessions(
          all
            .filter((s) => s.status === "ended")
            .sort((a, b) => (b.ended_at ?? 0) - (a.ended_at ?? 0))
            .slice(0, HISTORY_LIMIT)
        )
      )
      .catch(() => setSessions([]));
  }, []);

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | null = null;
    onSessionChanged(refresh).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [refresh]);

  function claudeSessionId(jsonlPath: string | null): string | undefined {
    if (!jsonlPath) return undefined;
    const base = jsonlPath.split(/[\\/]/).pop() ?? "";
    return base.endsWith(".jsonl") ? base.slice(0, -".jsonl".length) : undefined;
  }

  async function resume(s: Session) {
    try {
      await launchSession({
        project_dir: s.project_dir,
        model: s.model,
        resume: claudeSessionId(s.jsonl_path),
      });
      toast({ title: "Session resumed" });
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't resume", description: msg, variant: "destructive" });
    }
  }

  return (
    <div className="text-foreground">
      <div className="flex items-center gap-2 px-4 py-3 border-b border-border bg-background/55 backdrop-blur-xl">
        <button
          onClick={onBack}
          aria-label="Back"
          title="Back"
          className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
        >
          <ArrowLeft className="h-4 w-4" />
        </button>
        <div className="font-semibold tracking-tight">History</div>
      </div>
      <div className="p-4 min-h-[60vh]">
        {sessions === null ? (
          <div className="text-sm text-muted-foreground">Loading...</div>
        ) : sessions.length === 0 ? (
          <div className="text-sm text-muted-foreground">
            No ended sessions yet. Sessions appear here after you Kill them or claude exits.
          </div>
        ) : (
          <div className="space-y-2">
            <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground mb-3">
              Showing last {sessions.length} ended session{sessions.length === 1 ? "" : "s"}
            </div>
            {sessions.map((s, i) => {
              const projectName =
                s.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? s.project_dir;
              return (
                <div
                  key={s.id}
                  className="flex items-center gap-3 rounded-lg glass-panel p-3 animate-row-in"
                  style={{ animationDelay: `${i * 70}ms` }}
                >
                  <div className="h-2 w-2 rounded-full bg-[var(--status-stopped)] flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="font-semibold text-sm truncate">{projectName}</div>
                    <div className="text-xs text-muted-foreground truncate font-mono">
                      {s.project_dir}
                    </div>
                  </div>
                  {s.tokens_out > 0 && (
                    <div className="text-xs text-muted-foreground font-mono">
                      tokens: {fmtTokens(s.tokens_out)}
                    </div>
                  )}
                  <div className="text-xs text-muted-foreground font-mono">
                    {duration(s.started_at, s.ended_at)}
                  </div>
                  <div className="text-xs text-muted-foreground font-mono">
                    {s.ended_at ? relativeTime(s.ended_at) : ""}
                  </div>
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded-full border border-accent/35 text-accent bg-accent/10">
                    {s.model}
                  </span>
                  <Button size="sm" variant="ghost" onClick={() => resume(s)}>
                    {s.jsonl_path ? "Resume" : "Re-launch"}
                  </Button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Run the build**

```bash
npm run build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add src/components/History.tsx
git commit -m "feat(ui): glass History rows matching Dashboard shell"
```

---

## Task 20: Final verification — visual walkthrough in Tauri dev

**Files:** none (manual verification)

This is the eyeball pass. Run the dev shell, walk every surface, verify the spec checklist.

- [ ] **Step 1: Start the Tauri dev shell**

```bash
npm run tauri dev
```

Expected: window opens with the dark Warm Aurora theme. Aurora gradient is visible behind the chrome and is slowly drifting.

- [ ] **Step 2: Verify Dashboard (populated)**

Launch a session (or multiple). Confirm:
- Top bar is translucent over the aurora.
- Brand mark is the gradient square (not the raster icon).
- Launch button has the coral gradient with shadow.
- History/Settings are icon-only with tooltips.
- Each session row is a frosted panel with hairline border.
- Running session dots pulse (green, ripple effect).
- Idle session dots are static amber.
- Model badge is coral-tinted with mono font.
- Kill button is coral-outlined.
- Rows fade up on first mount with a stagger.

- [ ] **Step 3: Verify Dashboard (empty state)**

Kill all sessions. Confirm:
- Circular accent icon at top.
- "No running sessions" headline.
- Hotkey rendered as a `<kbd>` chip.
- Primary Launch button.

- [ ] **Step 4: Verify LaunchDialog keyboard handling**

Open the Launch dialog. Exercise each:
- Type a path, press Enter — launches.
- Click a recent — launches that recent.
- Press ↓ multiple times — highlight cycles through recents.
- Press ↑ — highlight goes back.
- Press Enter on a highlighted recent — launches that recent.
- Press Esc once — clears highlight (dialog stays open).
- Press Esc again — dialog closes.
- Open dialog, immediately type ↓Enter very fast — launches the first recent (no race).
- Highlight on active recent uses the coral gradient bar with left border (not the old solid fill).

If any of these fail, revert Task 16 (`git revert <sha>`) and re-apply only className changes.

- [ ] **Step 5: Verify UpdateBanner**

If an update is available (or a mocked one for testing), confirm:
- Slim coral gradient bar above the topbar.
- Glowing accent dot.
- "Restart & install" + dismiss (X) buttons.

- [ ] **Step 6: Verify Settings + theme toggle**

Open Settings. Confirm:
- Glass-panel sections with accent labels.
- Theme section toggles between dark and light.
- Toggling to light removes the aurora and makes panels solid.
- Reload the app while in light mode — opens in light mode (no flash).
- Toggle back to dark — aurora returns immediately.

- [ ] **Step 7: Verify Onboarding**

Clear first-run flag (delete the appropriate Tauri config or restart with a clean profile). Confirm:
- Glass card centered on aurora.
- Gradient brand mark + welcome.
- Three fields work; Get started saves and returns to Dashboard.

- [ ] **Step 8: Verify History**

Open History. Confirm:
- Same shell as Dashboard with back arrow.
- Glass rows with stopped (grey) dot.
- Resume / Re-launch button works.

- [ ] **Step 9: Verify toasts**

Trigger a success toast (Save settings) and a destructive toast (kill an already-killed session via stale UI). Confirm:
- Bottom-right glass toast.
- Default toast: glass-strong, foreground text.
- Destructive toast: coral-tinted destructive variant with frosted bg.

- [ ] **Step 10: Verify reduced motion**

Set the OS-level reduced motion preference (Windows: Settings → Accessibility → Visual effects → Animation effects off). Reload the app. Confirm:
- Aurora is static (no drift).
- Running dots don't pulse.
- Rows don't fade in.

Reset OS preference when done.

- [ ] **Step 11: Verify production build works**

```bash
npm run build
```

Expected: build succeeds. Open the built `dist/` artifact through Tauri's preview path or by re-running `tauri dev` (which uses the same code path).

- [ ] **Step 12: No commit needed (verification only).**

If issues are found, fix them in a follow-up task and commit.

---

## Self-review notes

**Spec coverage check:**
- Tokens (dark + light, why HEX, primary gradient layered): Tasks 1-2 ✓
- Aurora background: Task 5 + index.css (Task 2) ✓
- Motion (drift, pulse, row mount, reduced-motion): Task 2 (CSS), Task 13 (row stagger), Task 1 (animations registered) ✓
- Typography (Geist sans + mono, mono on paths/badges/inputs): Task 1 (config), Task 3 (HTML link), Tasks 9, 13, 16, 19 (mono usages) ✓
- Icons (Plus, History, Settings, X, ArrowLeft, plus Sun/Moon for theme toggle): Tasks 12, 14, 15, 18, 19 ✓
- Theme switching (synchronous bootstrap + Settings toggle): Task 4 + Task 18 ✓
- Component specs (Dashboard, SessionRow, EmptyState, LaunchDialog, UpdateBanner, Onboarding, Settings, History, Toaster): Tasks 12-19 ✓
- File-level change list: Tasks cover every listed file; nothing on the "Untouched" list is modified ✓
- Risks (LaunchDialog regression, theme flash, backdrop-filter, reduced motion, light theme): all addressed ✓

**Type consistency check:**
- `SessionRow` gains an optional `index?: number` prop (Task 13). `Dashboard.tsx` passes `index={i}` (Task 12). Optional prop, default 0, no breakage if a caller forgets. ✓
- `History.tsx` uses inline row markup (does not import `SessionRow`), so the new prop change does not affect it. ✓
- New CSS variables (`--border-strong`, `--primary-from`, `--primary-to`, `--aurora-1/2/3`, `--status-running/idle/stopped`) are all declared in Task 2 and used in subsequent tasks. ✓
- `aurora-layer`, `glass-panel`, `glass-panel-strong`, `dot-running-glow` CSS classes declared in Task 2; first used in Tasks 5, 7-19. ✓
- `animate-row-in` Tailwind animation registered in Task 1; used in Tasks 13 and 19. ✓

**Placeholder scan:** No "TBD" / "TODO" / "implement later" / "similar to Task N" / unspecified code. All steps contain exact code or exact replacement strings.

**Bite-size check:** Each task is one component/file with 2-5 minute steps. The largest tasks (12, 17, 18, 19) replace whole files because the deltas are larger than the surrounding context — keeping them whole avoids edit ambiguity.
