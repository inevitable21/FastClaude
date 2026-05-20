import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import type { Project, Session, Subtask } from "@/types";
import {
  focusSession,
  killSession,
  listProjects,
  setAutoContinue as setAutoContinueIpc,
  setSessionProject,
  setSessionTitle,
} from "@/lib/ipc";
import { ParentTodoBadge } from "./ParentTodoBadge";

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function fmtCountdown(targetEpoch: number): string {
  const secs = Math.max(0, Math.floor(targetEpoch - Date.now() / 1000));
  if (secs <= 0) return "now";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${secs}s`;
}

function elapsed(startedAt: number): string {
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}

function errMessage(e: unknown): string {
  if (typeof e === "string") return e;
  return (e as { message?: string })?.message ?? String(e);
}

const DEFAULT_TITLE = "Untitled";

export function SessionRow({
  session,
  onChange,
  index = 0,
  hideProjectName = false,
}: {
  session: Session;
  onChange: () => void;
  index?: number;
  hideProjectName?: boolean;
}) {
  const { toast } = useToast();
  const [subtaskText, setSubtaskText] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const [draftTitle, setDraftTitle] = useState(session.title);
  // Right-click menu state. menuPos === null means hidden.
  const [menuPos, setMenuPos] = useState<{ x: number; y: number } | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!session.subtask_id) {
      setSubtaskText(null);
      return;
    }
    let cancelled = false;
    invoke<Subtask>("get_subtask", { id: session.subtask_id })
      .then((s) => {
        if (!cancelled) setSubtaskText(s.text);
      })
      .catch(() => {
        if (!cancelled) setSubtaskText(null);
      });
    return () => {
      cancelled = true;
    };
  }, [session.subtask_id]);

  // Keep the draft in sync when the row's title changes from elsewhere.
  useEffect(() => {
    if (!editingTitle) setDraftTitle(session.title);
  }, [session.title, editingTitle]);

  // Close the context menu when clicking outside it or pressing Escape.
  useEffect(() => {
    if (menuPos === null) return;
    const onDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuPos(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuPos(null);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuPos]);

  const openMenu = useCallback(async (e: React.MouseEvent) => {
    e.preventDefault();
    setMenuPos({ x: e.clientX, y: e.clientY });
    try {
      setProjects(await listProjects());
    } catch {
      setProjects([]);
    }
  }, []);

  const dotClass =
    session.status === "running"
      ? "bg-[var(--status-running)] dot-running-glow"
      : session.status === "idle"
      ? "bg-[var(--status-idle)]"
      : "bg-[var(--status-stopped)]";

  const folderName =
    session.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? session.project_dir;
  // Title is the user-visible label. Falls back to the subtask text (for TODO
  // sessions when the project name is hidden) and then to the folder name so
  // legacy "Untitled" rows still have a sensible label.
  const fallbackLabel = hideProjectName
    ? subtaskText ?? `Session · started ${elapsed(session.started_at)} ago`
    : folderName;
  const displayTitle =
    session.title && session.title !== DEFAULT_TITLE ? session.title : fallbackLabel;

  async function focus() {
    try {
      await focusSession(session.id);
    } catch (e: unknown) {
      toast({
        title: "Couldn't focus session",
        description: errMessage(e),
        variant: "destructive",
      });
    }
    onChange();
  }
  async function kill() {
    try {
      await killSession(session.id);
    } catch (e: unknown) {
      toast({
        title: "Couldn't kill session",
        description: errMessage(e),
        variant: "destructive",
      });
    }
    onChange();
  }

  async function commitTitle() {
    const next = draftTitle.trim();
    if (!next || next === session.title) {
      setEditingTitle(false);
      setDraftTitle(session.title);
      return;
    }
    try {
      await setSessionTitle(session.id, next);
      toast({ title: "Title updated" });
    } catch (e: unknown) {
      toast({
        title: "Couldn't rename session",
        description: errMessage(e),
        variant: "destructive",
      });
      setDraftTitle(session.title);
    }
    setEditingTitle(false);
    onChange();
  }

  async function moveTo(projectId: string) {
    setMenuPos(null);
    if (projectId === session.project) return;
    try {
      await setSessionProject(session.id, projectId);
      toast({ title: "Session moved" });
    } catch (e: unknown) {
      toast({
        title: "Couldn't move session",
        description: errMessage(e),
        variant: "destructive",
      });
    }
    onChange();
  }

  return (
    <div
      className="flex items-center gap-3 rounded-lg glass-panel p-3 transition-colors hover:border-border-strong animate-row-in"
      style={{ animationDelay: `${index * 70}ms` }}
      onContextMenu={openMenu}
    >
      <div aria-hidden className={`h-2 w-2 rounded-full flex-shrink-0 ${dotClass}`} />
      <div className="flex-1 min-w-0">
        {editingTitle ? (
          <input
            autoFocus
            className="bg-transparent border-b border-accent outline-none text-sm font-semibold w-full"
            value={draftTitle}
            onChange={(e) => setDraftTitle(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") {
                setDraftTitle(session.title);
                setEditingTitle(false);
              }
            }}
          />
        ) : (
          <div
            className="font-semibold text-sm line-clamp-2 break-words cursor-text"
            onDoubleClick={() => {
              setDraftTitle(
                session.title && session.title !== DEFAULT_TITLE
                  ? session.title
                  : "",
              );
              setEditingTitle(true);
            }}
            title="Double-click to rename, right-click for more"
          >
            {displayTitle}
          </div>
        )}
        {session.subtask_id && <ParentTodoBadge subtaskId={session.subtask_id} />}
        {!hideProjectName && (
          <div className="text-xs text-muted-foreground truncate font-mono">
            {session.project_dir}
          </div>
        )}
      </div>
      {session.tokens_out > 0 && (
        <div className="text-xs text-muted-foreground font-mono">
          tokens: {fmtTokens(session.tokens_out)}
        </div>
      )}
      <div className="text-xs text-muted-foreground font-mono">{elapsed(session.started_at)}</div>
      <span className="text-[10px] font-mono px-2 py-0.5 rounded-full border border-accent/35 text-accent bg-accent/10">
        {session.model}
      </span>
      {(() => {
        const armed = session.auto_continue;
        const pending = session.next_resume_at !== null;
        const capReached = session.resume_count >= session.resume_cap;
        let label = "↻ off";
        let title = "Auto-continue is off. Click to arm.";
        let className = "border-border text-muted-foreground";
        if (armed && capReached) {
          label = "↻ cap";
          title = `Cap reached (${session.resume_count}/${session.resume_cap}). Toggle off and on to re-arm.`;
          className = "border-border text-muted-foreground opacity-60";
        } else if (armed && pending && session.next_resume_at !== null) {
          label = `↻ ${fmtCountdown(session.next_resume_at)}`;
          title = `Will resume at the reset (attempt ${session.resume_count + 1} of ${session.resume_cap}).`;
          className = "border-accent text-accent bg-accent/10";
        } else if (armed) {
          label = "↻ on";
          title = `Armed — will respawn when the 5h limit resets. (${session.resume_count}/${session.resume_cap} used)`;
          className = "border-accent text-accent bg-accent/10";
        }
        async function toggle() {
          try {
            await setAutoContinueIpc(session.id, !armed);
          } catch (e: unknown) {
            const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
            toast({ title: "Couldn't change auto-continue", description: msg, variant: "destructive" });
          }
          onChange();
        }
        return (
          <button
            title={title}
            onClick={toggle}
            className={`text-[10px] font-mono px-2 py-0.5 rounded-full border ${className} hover:brightness-110`}
          >
            {label}
          </button>
        );
      })()}
      <Button size="sm" variant="ghost" onClick={focus}>
        Focus
      </Button>
      <Button size="sm" variant="destructive" onClick={kill}>
        Kill
      </Button>
      {menuPos && (
        <div
          ref={menuRef}
          className="fixed z-50 min-w-[200px] rounded-md border border-border input-fill shadow-lg py-1"
          style={{ left: menuPos.x, top: menuPos.y }}
        >
          <div className="px-2 py-1 text-[10px] uppercase tracking-[0.12em] text-muted-foreground">
            Move to project
          </div>
          {projects.length === 0 ? (
            <div className="px-2 py-1 text-xs text-muted-foreground">No projects yet</div>
          ) : (
            projects.map((p) => (
              <button
                key={p.id}
                onClick={() => moveTo(p.id)}
                className={`block w-full text-left px-2 py-1 text-xs hover:bg-foreground/[0.06] ${
                  p.id === session.project ? "text-accent" : ""
                }`}
              >
                {p.display_name}
                {p.id === session.project && (
                  <span className="ml-1 text-[10px] text-muted-foreground">(current)</span>
                )}
              </button>
            ))
          )}
          <div className="border-t border-border mt-1 pt-1">
            <button
              onClick={() => {
                setMenuPos(null);
                setDraftTitle(
                  session.title && session.title !== DEFAULT_TITLE
                    ? session.title
                    : "",
                );
                setEditingTitle(true);
              }}
              className="block w-full text-left px-2 py-1 text-xs hover:bg-foreground/[0.06]"
            >
              Rename…
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
