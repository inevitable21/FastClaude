import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useToast } from "@/hooks/use-toast";
import {
  listSessions,
  listProjects,
  onProjectChanged,
  onSessionChanged,
  getConfig,
  onAutoContinueFired,
  onAutoContinueFailed,
  onAutoContinueGaveUp,
} from "@/lib/ipc";
import type { Project, Session } from "@/types";
import { SessionRow } from "./SessionRow";
import { LaunchDialog } from "./LaunchDialog";
import { EmptyState } from "./EmptyState";

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

// Match Rust-side normalize_project_dir so groups merge across slash style
// and case (Windows paths are case-insensitive). Mirrors History.tsx and
// ProjectPane.tsx so a session's project_dir lines up with Project.norm_path.
function projectKey(dir: string): string {
  return dir.replace(/\\/g, "/").toLowerCase().replace(/\/+$/, "");
}

interface SessionGroup {
  key: string;
  projectDir: string;
  displayName: string;
  sessions: Session[];
  latestStartedAt: number;
  totalTokensOut: number;
}

export function Dashboard({
  launchOpen,
  setLaunchOpen,
}: {
  launchOpen: boolean;
  setLaunchOpen: (v: boolean) => void;
}) {
  const { toast } = useToast();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [hotkey, setHotkey] = useState<string>("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

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

  const refreshProjects = useCallback(() => {
    listProjects()
      .then(setProjects)
      .catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    refresh();
    refreshProjects();
    const unlisteners: Array<() => void> = [];
    onSessionChanged(refresh).then((fn) => unlisteners.push(fn));
    onProjectChanged(refreshProjects).then((fn) => unlisteners.push(fn));
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
  }, [refresh, refreshProjects, toast]);

  const groups = useMemo<SessionGroup[]>(() => {
    const projectByKey = new Map<string, Project>();
    for (const p of projects) projectByKey.set(p.norm_path, p);
    const map = new Map<string, SessionGroup>();
    for (const s of sessions) {
      const key = projectKey(s.project_dir);
      let g = map.get(key);
      if (!g) {
        const matched = projectByKey.get(key);
        const fallback =
          s.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? s.project_dir;
        g = {
          key,
          projectDir: s.project_dir,
          displayName: matched?.display_name ?? fallback,
          sessions: [],
          latestStartedAt: 0,
          totalTokensOut: 0,
        };
        map.set(key, g);
      }
      g.sessions.push(s);
      g.latestStartedAt = Math.max(g.latestStartedAt, s.started_at);
      g.totalTokensOut += s.tokens_out;
    }
    for (const g of map.values()) {
      g.sessions.sort((a, b) => b.started_at - a.started_at);
    }
    return Array.from(map.values()).sort(
      (a, b) => b.latestStartedAt - a.latestStartedAt,
    );
  }, [sessions, projects]);

  function toggleGroup(key: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  return (
    <div className="text-foreground">
      <div className="p-4 min-h-[60vh]">
        {sessions.length === 0 ? (
          <EmptyState onLaunch={() => setLaunchOpen(true)} hotkey={hotkey} />
        ) : (
          <>
            <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground mb-3">
              {sessions.length} running session{sessions.length === 1 ? "" : "s"} across{" "}
              {groups.length} project{groups.length === 1 ? "" : "s"}
            </div>
            <div className="space-y-2">
              {groups.map((g, gi) => {
                const open = !collapsed.has(g.key);
                return (
                  <div
                    key={g.key}
                    className="rounded-lg glass-panel animate-row-in overflow-hidden"
                    style={{ animationDelay: `${gi * 70}ms` }}
                  >
                    <button
                      onClick={() => toggleGroup(g.key)}
                      aria-expanded={open}
                      className="flex w-full items-center gap-3 px-3 py-3 text-left transition-colors hover:bg-foreground/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                    >
                      {open ? (
                        <ChevronDown className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                      ) : (
                        <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="font-semibold text-sm truncate">
                          {g.displayName}
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
                    </button>
                    {open && (
                      <div className="border-t border-border p-2 space-y-2">
                        {g.sessions.length === 0 ? (
                          <div className="text-xs text-muted-foreground px-2 py-3">
                            No sessions in this project.
                          </div>
                        ) : (
                          g.sessions.map((s, i) => (
                            <SessionRow
                              key={s.id}
                              session={s}
                              onChange={refresh}
                              index={i}
                              hideProjectName
                            />
                          ))
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>
      <LaunchDialog open={launchOpen} onOpenChange={setLaunchOpen} onLaunched={refresh} />
    </div>
  );
}
