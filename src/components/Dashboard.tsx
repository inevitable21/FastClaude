import { useCallback, useEffect, useState } from "react";
import { listSessions, onSessionChanged } from "@/lib/ipc";
import type { Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";

export function Dashboard() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [open, setOpen] = useState(false);

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
    <div className="min-h-screen bg-background text-foreground">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
        <div className="font-semibold">FastClaude</div>
        <button
          onClick={() => setOpen(true)}
          className="ml-auto px-3 py-1.5 rounded bg-primary text-primary-foreground text-sm"
        >
          + Launch new session
        </button>
      </div>
      <div className="p-4">
        {sessions.length === 0 ? (
          <EmptyState onLaunch={() => setOpen(true)} />
        ) : (
          <>
            <div className="text-xs text-muted-foreground mb-2">
              {sessions.length} running session{sessions.length === 1 ? "" : "s"}
            </div>
            <div className="space-y-2">
              {sessions.map((s) => (
                <SessionRow key={s.id} session={s} onChange={refresh} />
              ))}
            </div>
          </>
        )}
      </div>
      <LaunchDialog open={open} onOpenChange={setOpen} onLaunched={refresh} />
    </div>
  );
}
