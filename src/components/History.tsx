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
          className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
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
                  <div aria-hidden className="h-2 w-2 rounded-full bg-[var(--status-stopped)] flex-shrink-0" />
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
