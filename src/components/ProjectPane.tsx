import { useCallback, useEffect, useState } from "react";
import { listSessions, onSessionChanged, setProjectName } from "@/lib/ipc";
import { TodoList } from "./TodoList";
import { TodoDialog } from "./TodoDialog";
import { SubtaskReviewDialog } from "./SubtaskReviewDialog";
import { SessionRow } from "./SessionRow";
import type { Project, Session } from "@/types";
import { Pencil } from "lucide-react";

interface Props {
  project: Project;
  onLaunch: () => void; // opens the existing LaunchDialog
}

export function ProjectPane({ project, onLaunch }: Props) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [todoDialogOpen, setTodoDialogOpen] = useState(false);
  const [reviewTodoId, setReviewTodoId] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(project.display_name);

  const refresh = useCallback(() => {
    listSessions().then((all) => {
      const norm = project.norm_path;
      setSessions(
        all.filter(
          (s) => s.project_dir.replace(/\\/g, "/").toLowerCase().replace(/\/+$/, "") === norm,
        ),
      );
    }).catch(() => setSessions([]));
  }, [project.norm_path]);

  useEffect(() => {
    refresh();
    const u: Array<() => void> = [];
    onSessionChanged(refresh).then((fn) => u.push(fn));
    const t = setInterval(refresh, 5000);
    return () => { u.forEach((fn) => fn()); clearInterval(t); };
  }, [refresh]);

  return (
    <div className="flex-1 p-4 space-y-4 overflow-y-auto">
      <header className="flex items-center justify-between">
        {editing ? (
          <input
            autoFocus
            className="bg-transparent border-b border-accent outline-none text-lg font-semibold"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={() => {
              if (draftName.trim()) setProjectName(project.id, draftName.trim());
              setEditing(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") { setDraftName(project.display_name); setEditing(false); }
            }}
          />
        ) : (
          <h2 className="text-lg font-semibold flex items-center gap-2">
            {project.display_name}
            <button onClick={() => setEditing(true)} className="opacity-50 hover:opacity-100">
              <Pencil className="h-3 w-3" />
            </button>
          </h2>
        )}
        <div className="flex gap-2">
          <button className="text-xs px-2 py-1 border border-border rounded" onClick={() => setTodoDialogOpen(true)}>
            + TODO
          </button>
          <button className="text-xs px-2 py-1 border border-border rounded" onClick={onLaunch}>
            + Launch session
          </button>
        </div>
      </header>
      <TodoList
        projectId={project.id}
        onOpenTodo={setReviewTodoId}
        onAddTodo={() => setTodoDialogOpen(true)}
      />
      <section className="space-y-2">
        <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">Sessions</div>
        {sessions.length === 0 ? (
          <div className="text-xs text-muted-foreground">No sessions in this project.</div>
        ) : (
          sessions.map((s, i) => <SessionRow key={s.id} session={s} onChange={refresh} index={i} />)
        )}
      </section>
      <TodoDialog
        open={todoDialogOpen}
        projectId={project.id}
        onOpenChange={setTodoDialogOpen}
        onCreated={(id) => setReviewTodoId(id)}
      />
      <SubtaskReviewDialog
        todoId={reviewTodoId}
        open={reviewTodoId !== null}
        onOpenChange={(o) => { if (!o) setReviewTodoId(null); }}
      />
    </div>
  );
}
