import { useCallback, useEffect, useState } from "react";
import { listSessions, onSessionChanged, getConfig } from "@/lib/ipc";
import type { Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";

export function Dashboard({
  launchOpen,
  setLaunchOpen,
}: {
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
