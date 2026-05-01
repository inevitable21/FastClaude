import { useCallback, useEffect } from "react";
import { useState } from "react";
import { Plus, History as HistoryIcon, Settings as SettingsIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
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
        <Button onClick={() => setLaunchOpen(true)}>
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
