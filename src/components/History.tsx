import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useToast } from "@/hooks/use-toast";
import {
  clearEndedSessions,
  deleteSession,
  deleteSessions,
  launchSession,
  listAllSessions,
  onSessionChanged,
} from "@/lib/ipc";
import type { Session } from "@/types";

const HISTORY_LIMIT = 200;

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

// Match the Rust-side normalize_project_dir so groups merge across slash style
// and case differences (Windows paths are case-insensitive).
function projectKey(dir: string): string {
  return dir.replace(/\\/g, "/").toLowerCase().replace(/\/+$/, "");
}

interface Group {
  key: string;
  projectDir: string;
  sessions: Session[];
  latestEndedAt: number;
  totalTokensOut: number;
}

export function History({ onBack: _onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [openGroups, setOpenGroups] = useState<Set<string>>(new Set());
  const [pendingDelete, setPendingDelete] = useState<
    { kind: "all" } | { kind: "group"; group: Group } | null
  >(null);

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

  const groups = useMemo<Group[]>(() => {
    if (!sessions) return [];
    const map = new Map<string, Group>();
    for (const s of sessions) {
      const k = projectKey(s.project_dir);
      let g = map.get(k);
      if (!g) {
        g = {
          key: k,
          projectDir: s.project_dir,
          sessions: [],
          latestEndedAt: 0,
          totalTokensOut: 0,
        };
        map.set(k, g);
      }
      g.sessions.push(s);
      g.latestEndedAt = Math.max(g.latestEndedAt, s.ended_at ?? 0);
      g.totalTokensOut += s.tokens_out;
    }
    return Array.from(map.values()).sort(
      (a, b) => b.latestEndedAt - a.latestEndedAt
    );
  }, [sessions]);

  function toggleGroup(key: string) {
    setOpenGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

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

  async function removeOne(s: Session) {
    try {
      await deleteSession(s.id);
      refresh();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't delete", description: msg, variant: "destructive" });
    }
  }

  async function confirmDelete() {
    if (!pendingDelete) return;
    try {
      const n =
        pendingDelete.kind === "all"
          ? await clearEndedSessions()
          : await deleteSessions(pendingDelete.group.sessions.map((s) => s.id));
      setPendingDelete(null);
      toast({ title: `Cleared ${n} session${n === 1 ? "" : "s"}` });
      refresh();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't delete", description: msg, variant: "destructive" });
    }
  }

  const totalSessions = sessions?.length ?? 0;

  return (
    <div className="text-foreground">
      <div className="p-4 min-h-[60vh]">
        {sessions === null ? (
          <div className="text-sm text-muted-foreground">Loading...</div>
        ) : groups.length === 0 ? (
          <div className="text-sm text-muted-foreground">
            No ended sessions yet. Sessions appear here after you Kill them or claude exits.
          </div>
        ) : (
          <div className="space-y-2">
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
            {groups.map((g, gi) => {
              const open = openGroups.has(g.key);
              const projectName =
                g.projectDir.split(/[\\/]/).filter(Boolean).pop() ?? g.projectDir;
              return (
                <div
                  key={g.key}
                  className="rounded-lg glass-panel animate-row-in overflow-hidden"
                  style={{ animationDelay: `${gi * 70}ms` }}
                >
                  <div className="flex items-stretch group/header">
                    <button
                      onClick={() => toggleGroup(g.key)}
                      aria-expanded={open}
                      className="flex flex-1 min-w-0 items-center gap-3 px-3 py-3 text-left transition-colors hover:bg-foreground/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                    >
                      {open ? (
                        <ChevronDown className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                      ) : (
                        <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="font-semibold text-sm truncate">
                          {projectName}
                          <span className="ml-2 text-[10px] font-mono px-1.5 py-0.5 rounded-full border border-border bg-foreground/[0.04] text-muted-foreground align-middle">
                            {g.sessions.length}
                          </span>
                        </div>
                        <div className="text-xs text-muted-foreground truncate font-mono">
                          {g.projectDir}
                        </div>
                      </div>
                      {g.totalTokensOut > 0 && (
                        <div className="text-xs text-muted-foreground font-mono whitespace-nowrap">
                          tokens: {fmtTokens(g.totalTokensOut)}
                        </div>
                      )}
                      <div className="text-xs text-muted-foreground font-mono whitespace-nowrap">
                        last {relativeTime(g.latestEndedAt)}
                      </div>
                    </button>
                    <button
                      onClick={() => setPendingDelete({ kind: "group", group: g })}
                      aria-label={`Delete all ${g.sessions.length} sessions for ${projectName}`}
                      title="Delete folder from history"
                      className="flex-shrink-0 inline-flex w-10 items-center justify-center text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  {open && (
                    <div className="border-t border-border divide-y divide-border/60">
                      {g.sessions.map((s) => (
                        <div
                          key={s.id}
                          className="flex items-center gap-3 pl-9 pr-3 py-2.5 text-xs"
                        >
                          <div
                            aria-hidden
                            className="h-2 w-2 rounded-full bg-[var(--status-stopped)] flex-shrink-0"
                          />
                          <div className="flex-1 min-w-0 font-mono text-muted-foreground">
                            {duration(s.started_at, s.ended_at)} ·{" "}
                            {s.ended_at ? relativeTime(s.ended_at) : "—"}
                          </div>
                          {s.tokens_out > 0 && (
                            <div className="font-mono text-muted-foreground whitespace-nowrap">
                              {fmtTokens(s.tokens_out)}
                            </div>
                          )}
                          <span className="text-[10px] font-mono px-2 py-0.5 rounded-full border border-accent/35 text-accent bg-accent/10 whitespace-nowrap">
                            {s.model}
                          </span>
                          <Button size="sm" variant="ghost" onClick={() => resume(s)}>
                            {s.jsonl_path ? "Resume" : "Re-launch"}
                          </Button>
                          <button
                            onClick={() => removeOne(s)}
                            aria-label="Delete session"
                            title="Delete from history"
                            className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
      <Dialog
        open={pendingDelete !== null}
        onOpenChange={(v) => !v && setPendingDelete(null)}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {pendingDelete?.kind === "group"
                ? `Delete history for "${pendingDelete.group.projectDir.split(/[\\/]/).filter(Boolean).pop() ?? pendingDelete.group.projectDir}"?`
                : "Clear all history?"}
            </DialogTitle>
          </DialogHeader>
          <div className="text-sm text-muted-foreground">
            {pendingDelete?.kind === "group" ? (
              <>
                Removes {pendingDelete.group.sessions.length} ended session
                {pendingDelete.group.sessions.length === 1 ? "" : "s"} for this folder.
              </>
            ) : (
              <>
                Removes {totalSessions} ended session{totalSessions === 1 ? "" : "s"} from
                FastClaude's history. Active sessions are kept.
              </>
            )}{" "}
            Claude's own session logs in <span className="font-mono">~/.claude/projects</span> are
            not affected — you can still resume by re-launching the same folder.
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setPendingDelete(null)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={confirmDelete}>
              {pendingDelete?.kind === "group" ? "Delete folder" : "Clear all"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
