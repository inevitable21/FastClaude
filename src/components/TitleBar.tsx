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
