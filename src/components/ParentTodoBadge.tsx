import { useEffect, useState } from "react";
import { Square } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { Subtask, Todo } from "@/types";

/* No public `get_subtask` IPC command exists for the frontend in v1 — we
   piggyback on `list_subtasks` by walking the todos. For badge display we
   only need the parent todo title and the ordinal i/N. */
async function loadBadgeData(subtaskId: string): Promise<{ todo: Todo; ord: number; total: number } | null> {
  // The IPC layer only exposes list_subtasks(todo_id). We don't know the
  // todo_id from the session alone — so the session row stores subtask_id;
  // a subsequent `list_subtasks` lookup across all todos would be wasteful.
  // For v1 we expose a tiny `get_subtask` IPC command (added in Task 14)
  // and call it directly via `invoke`.
  try {
    const subtask = await invoke<Subtask>("get_subtask", { id: subtaskId });
    const todo = await invoke<Todo>("get_todo", { id: subtask.todo_id });
    const siblings = await invoke<Subtask[]>("list_subtasks", { todoId: subtask.todo_id });
    return { todo, ord: subtask.ord + 1, total: siblings.length };
  } catch {
    return null;
  }
}

interface Props {
  subtaskId: string;
}

export function ParentTodoBadge({ subtaskId }: Props) {
  const [data, setData] = useState<{ todo: Todo; ord: number; total: number } | null>(null);

  useEffect(() => {
    let cancelled = false;
    loadBadgeData(subtaskId).then((d) => { if (!cancelled) setData(d); });
    return () => { cancelled = true; };
  }, [subtaskId]);

  if (!data) return null;
  const truncated = data.todo.title.length > 32 ? data.todo.title.slice(0, 30) + "…" : data.todo.title;
  return (
    <span
      className="inline-flex items-center gap-1 text-[10px] text-muted-foreground border border-border rounded px-1 py-[1px]"
      title={data.todo.title}
    >
      <Square className="h-2.5 w-2.5" />
      {truncated} • {data.ord}/{data.total}
    </span>
  );
}
