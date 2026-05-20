import { useCallback, useEffect, useState } from "react";
import { Check, X, RotateCw } from "lucide-react";
import {
  listTodos,
  markTodoFinished,
  dismissAutoSuggest,
  planTodo,
  onTodoChanged,
} from "@/lib/ipc";
import type { Todo } from "@/types";

interface Props {
  projectId: string;
  onOpenTodo: (todoId: string) => void;
  onAddTodo: () => void;
}

type Tab = "ongoing" | "finished";

export function TodoList({ projectId, onOpenTodo, onAddTodo }: Props) {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [tab, setTab] = useState<Tab>("ongoing");

  const refresh = useCallback(() => {
    listTodos(projectId).then(setTodos).catch(() => setTodos([]));
  }, [projectId]);

  useEffect(() => {
    refresh();
    const u: Array<() => void> = [];
    onTodoChanged(refresh).then((fn) => u.push(fn));
    return () => u.forEach((fn) => fn());
  }, [refresh]);

  const visible = todos.filter((t) =>
    tab === "ongoing" ? t.state !== "finished" : t.state === "finished",
  );

  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between">
        <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground">TODOs</div>
        <div className="flex items-center gap-2">
          <button
            className={`text-xs px-2 py-1 rounded ${tab === "ongoing" ? "bg-foreground/10" : ""}`}
            onClick={() => setTab("ongoing")}
          >
            Ongoing
          </button>
          <button
            className={`text-xs px-2 py-1 rounded ${tab === "finished" ? "bg-foreground/10" : ""}`}
            onClick={() => setTab("finished")}
          >
            Finished
          </button>
          <button className="text-xs px-2 py-1 border border-border rounded" onClick={onAddTodo}>
            + TODO
          </button>
        </div>
      </div>
      {visible.length === 0 ? (
        <div className="text-xs text-muted-foreground py-4">No {tab} TODOs.</div>
      ) : (
        <ul className="space-y-1">
          {visible.map((t) => (
            <li
              key={t.id}
              className="flex items-center justify-between px-2 py-1 border border-border rounded hover:bg-foreground/5 cursor-pointer"
              onClick={() => onOpenTodo(t.id)}
            >
              <div className="flex items-center gap-2">
                <span className={`inline-block w-2 h-2 rounded-full ${
                  t.state === "ongoing" ? "bg-accent" :
                  t.state === "finished" ? "bg-emerald-500" : "bg-muted-foreground"
                }`} />
                <span>{t.title}</span>
                {t.planner_status === "planning" && (
                  <span className="text-[10px] text-muted-foreground">planning…</span>
                )}
                {t.planner_status === "planner_failed" && (
                  <span
                    className="text-[10px] text-destructive cursor-help"
                    title={t.planner_error ?? "planner failed"}
                  >
                    planner failed
                  </span>
                )}
                {t.planner_status === "planner_failed" && (
                  <button
                    title="Retry planning"
                    className="text-[10px] text-muted-foreground hover:text-foreground"
                    onClick={(e) => { e.stopPropagation(); planTodo(t.id); }}
                  >
                    <RotateCw className="h-3 w-3" />
                  </button>
                )}
              </div>
              {t.auto_suggest_done_at && t.state !== "finished" && (
                <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                  <span className="text-[10px] text-muted-foreground">Done?</span>
                  <button
                    title="Mark finished"
                    className="hover:text-emerald-500"
                    onClick={() => markTodoFinished(t.id)}
                  >
                    <Check className="h-3 w-3" />
                  </button>
                  <button
                    title="Not yet"
                    className="hover:text-destructive"
                    onClick={() => dismissAutoSuggest(t.id)}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
