import { useCallback, useEffect, useState } from "react";
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

  async function relaunch(s: Session) {
    try {
      await launchSession({ project_dir: s.project_dir, model: s.model });
      toast({ title: "Session launched" });
    } catch (e: unknown) {
      const msg =
        typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't relaunch", description: msg, variant: "destructive" });
    }
  }

  return (
    <div className="bg-background text-foreground">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
        <button onClick={onBack} className="text-sm hover:underline">
          ← Back
        </button>
        <div className="font-semibold">History</div>
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
            <div className="text-xs text-muted-foreground mb-2">
              Showing last {sessions.length} ended session{sessions.length === 1 ? "" : "s"}
            </div>
            {sessions.map((s) => {
              const projectName =
                s.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? s.project_dir;
              return (
                <div
                  key={s.id}
                  className="flex items-center gap-3 rounded-lg border border-border p-3"
                >
                  <div className="h-2 w-2 rounded-full bg-zinc-400" />
                  <div className="flex-1 min-w-0">
                    <div className="font-semibold text-sm truncate">{projectName}</div>
                    <div className="text-xs text-muted-foreground truncate">
                      {s.project_dir}
                    </div>
                  </div>
                  {s.tokens_out > 0 && (
                    <div className="text-xs text-muted-foreground">
                      tokens: {fmtTokens(s.tokens_out)}
                    </div>
                  )}
                  <div className="text-xs text-muted-foreground">
                    {duration(s.started_at, s.ended_at)}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {s.ended_at ? relativeTime(s.ended_at) : ""}
                  </div>
                  <span className="text-xs px-2 py-0.5 rounded bg-blue-100 text-blue-800">
                    {s.model}
                  </span>
                  <Button size="sm" onClick={() => relaunch(s)}>
                    Re-launch
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
