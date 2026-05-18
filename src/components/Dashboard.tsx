import { useCallback, useEffect, useState } from "react";
import { useToast } from "@/hooks/use-toast";
import {
  listSessions,
  onSessionChanged,
  getConfig,
  onAutoContinueFired,
  onAutoContinueFailed,
  onAutoContinueGaveUp,
} from "@/lib/ipc";
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
  const { toast } = useToast();
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
    const unlisteners: Array<() => void> = [];
    onSessionChanged(refresh).then((fn) => unlisteners.push(fn));
    onAutoContinueFired((id) => {
      refresh();
      listSessions().then((all) => {
        const s = all.find((x) => x.id === id);
        const name = s?.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? "session";
        toast({ title: `Auto-continued ${name}` });
      });
    }).then((fn) => unlisteners.push(fn));
    onAutoContinueFailed(({ id, error }) => {
      refresh();
      toast({
        title: "Auto-continue failed",
        description: `${id.slice(0, 8)}…: ${error}`,
        variant: "destructive",
      });
    }).then((fn) => unlisteners.push(fn));
    onAutoContinueGaveUp((id) => {
      refresh();
      toast({
        title: "Auto-continue gave up",
        description: `${id.slice(0, 8)}… — 3 spawn failures in a row.`,
        variant: "destructive",
      });
    }).then((fn) => unlisteners.push(fn));
    const t = setInterval(refresh, 5000);
    return () => {
      for (const u of unlisteners) u();
      clearInterval(t);
    };
  }, [refresh, toast]);

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
