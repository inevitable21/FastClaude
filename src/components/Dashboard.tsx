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
  setSessionProject,
} from "@/lib/ipc";
import type { Project, Session } from "@/types";
import { DRAG_MIME, SessionRow } from "./SessionRow";
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
  // Stable identity for keying React + the collapsed Set.
  key: string;
  // Display path for the group header (the project's norm_path, or the
  // session's project_dir for orphan groups).
  projectDir: string;
  displayName: string;
  sessions: Session[];
  // 0 when the group is empty — used to push empty groups below the active
  // ones in the sort order.
  latestStartedAt: number;
  totalTokensOut: number;
  // True for groups synthesised from sessions whose explicit `project` id
  // and `project_dir` don't match any known project. Defensive — launching
  // should always upsert a Project — but better than dropping rows silently.
  orphan: boolean;
  // Project id when the group is backed by a real Project. null for orphan
  // groups — those have nowhere to drop onto.
  projectId: string | null;
}

export function Dashboard({ onLaunch }: { onLaunch: () => void }) {
  const { toast } = useToast();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [hotkey, setHotkey] = useState<string>("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  // Group key currently highlighted as a drop target.
  const [dropTargetKey, setDropTargetKey] = useState<string | null>(null);

  function hasSessionDrag(e: React.DragEvent): boolean {
    return Array.from(e.dataTransfer.types).includes(DRAG_MIME);
  }

  async function moveSessionToProject(sessionId: string, g: SessionGroup) {
    if (!g.projectId) return;
    // Don't move if the session is already in the target group — the row's
    // current `project` may differ from `projectId` for legacy "Default Project"
    // rows, but the UI treats them as already-grouped.
    const current = sessions.find((s) => s.id === sessionId);
    if (current && current.project === g.projectId) return;
    try {
      await setSessionProject(sessionId, g.projectId);
      toast({ title: `Moved to ${g.displayName}` });
    } catch (err) {
      toast({
        title: "Couldn't move session",
        description:
          typeof err === "string"
            ? err
            : (err as { message?: string })?.message ?? String(err),
        variant: "destructive",
      });
    }
  }

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
    // Seed one group per known project so projects with zero sessions still
    // appear (and surface the per-project empty state). Lookups by id (the
    // primary key for an explicit assignment) and by norm_path (the
    // path-based fallback for legacy "Default Project" rows — same rule
    // ProjectPane.tsx uses).
    const groupById = new Map<string, SessionGroup>();
    const groupByPath = new Map<string, SessionGroup>();
    for (const p of projects) {
      const g: SessionGroup = {
        key: `project:${p.id}`,
        projectDir: p.norm_path,
        displayName: p.display_name,
        sessions: [],
        latestStartedAt: 0,
        totalTokensOut: 0,
        orphan: false,
        projectId: p.id,
      };
      groupById.set(p.id, g);
      groupByPath.set(p.norm_path, g);
    }

    // Orphan bucket: sessions whose `project` id and `project_dir` don't
    // map to any visible project (defensive — launching auto-upserts a
    // Project, so this should usually stay empty).
    const orphanByKey = new Map<string, SessionGroup>();
    function orphanGroupFor(s: Session): SessionGroup {
      const k = projectKey(s.project_dir);
      let g = orphanByKey.get(k);
      if (!g) {
        const fallback =
          s.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? s.project_dir;
        g = {
          key: `orphan:${k}`,
          projectDir: s.project_dir,
          displayName: fallback,
          sessions: [],
          latestStartedAt: 0,
          totalTokensOut: 0,
          orphan: true,
          projectId: null,
        };
        orphanByKey.set(k, g);
      }
      return g;
    }

    for (const s of sessions) {
      // Primary: explicit project id assignment (e.g. set via the row's
      // right-click "Move to project" menu). Falls back to path-based
      // matching for legacy "Default Project" rows so they group sensibly
      // before being touched. Mirrors ProjectPane.tsx's filter logic.
      let g = groupById.get(s.project);
      if (!g && s.project === "Default Project") {
        g = groupByPath.get(projectKey(s.project_dir));
      }
      if (!g) g = orphanGroupFor(s);

      g.sessions.push(s);
      g.latestStartedAt = Math.max(g.latestStartedAt, s.started_at);
      g.totalTokensOut += s.tokens_out;
    }

    const all: SessionGroup[] = [
      ...Array.from(groupById.values()),
      ...Array.from(orphanByKey.values()),
    ];
    for (const g of all) {
      g.sessions.sort((a, b) => b.started_at - a.started_at);
    }
    // Sort order: groups with sessions first (most-recent activity wins),
    // then empty projects alphabetically, then orphan groups last.
    return all.sort((a, b) => {
      const aActive = a.sessions.length > 0;
      const bActive = b.sessions.length > 0;
      if (aActive !== bActive) return aActive ? -1 : 1;
      if (aActive) return b.latestStartedAt - a.latestStartedAt;
      if (a.orphan !== b.orphan) return a.orphan ? 1 : -1;
      return a.displayName.localeCompare(b.displayName);
    });
  }, [sessions, projects]);

  function toggleGroup(key: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  const activeGroupCount = groups.reduce(
    (n, g) => n + (g.sessions.length > 0 ? 1 : 0),
    0,
  );
  const hasAnyProject = projects.length > 0 || groups.some((g) => g.orphan);

  return (
    <div className="text-foreground">
      <div className="p-4 min-h-[60vh]">
        {!hasAnyProject ? (
          <div className="flex flex-col items-center justify-center py-16 text-center gap-3">
            <h2 className="text-lg font-semibold">No projects yet</h2>
            <p className="text-sm text-muted-foreground max-w-sm">
              Add a project from the sidebar, or launch a session — FastClaude
              will track the folder as a project automatically.
            </p>
          </div>
        ) : sessions.length === 0 ? (
          <EmptyState onLaunch={onLaunch} hotkey={hotkey} />
        ) : (
          <>
            <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground mb-3">
              {sessions.length} running session{sessions.length === 1 ? "" : "s"} across{" "}
              {activeGroupCount} project{activeGroupCount === 1 ? "" : "s"}
            </div>
            <div className="space-y-2">
              {groups.map((g, gi) => {
                const open = !collapsed.has(g.key);
                const empty = g.sessions.length === 0;
                const droppable = g.projectId !== null;
                const dropping = dropTargetKey === g.key;
                return (
                  <div
                    key={g.key}
                    className={`rounded-lg glass-panel animate-row-in overflow-hidden ${
                      empty ? "opacity-70" : ""
                    } ${dropping ? "ring-2 ring-accent" : ""}`}
                    style={{ animationDelay: `${gi * 70}ms` }}
                    onDragOver={(e) => {
                      if (!droppable || !hasSessionDrag(e)) return;
                      e.preventDefault();
                      e.dataTransfer.dropEffect = "move";
                      if (dropTargetKey !== g.key) setDropTargetKey(g.key);
                    }}
                    onDragLeave={(e) => {
                      if (e.currentTarget.contains(e.relatedTarget as Node)) return;
                      if (dropTargetKey === g.key) setDropTargetKey(null);
                    }}
                    onDrop={(e) => {
                      if (!droppable) return;
                      e.preventDefault();
                      setDropTargetKey(null);
                      const sessionId = e.dataTransfer.getData(DRAG_MIME);
                      if (sessionId) moveSessionToProject(sessionId, g);
                    }}
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
                        <div className="font-semibold text-sm truncate flex items-center gap-2">
                          <span className="truncate">{g.displayName}</span>
                          <span
                            className={`text-[10px] font-mono px-1.5 py-0.5 rounded-full border align-middle ${
                              empty
                                ? "border-border/60 bg-transparent text-muted-foreground/70"
                                : "border-border bg-foreground/[0.04] text-muted-foreground"
                            }`}
                          >
                            {g.sessions.length}
                          </span>
                          {g.orphan && (
                            <span
                              className="text-[10px] font-mono px-1.5 py-0.5 rounded-full border border-border/60 text-muted-foreground/70"
                              title="No matching project — using folder path"
                            >
                              orphan
                            </span>
                          )}
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
                        {empty ? (
                          <div className="text-xs text-muted-foreground px-2 py-3">
                            No active sessions in this project.
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
    </div>
  );
}
