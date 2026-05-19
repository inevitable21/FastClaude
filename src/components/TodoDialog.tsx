import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { createTodo, planTodo } from "@/lib/ipc";

interface Props {
  open: boolean;
  projectId: string;
  onOpenChange: (open: boolean) => void;
  onCreated: (todoId: string) => void;
}

export function TodoDialog({ open, projectId, onOpenChange, onCreated }: Props) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function submit() {
    if (!title.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      const todo = await createTodo(projectId, title.trim());
      // Fire-and-forget: planner runs in the background. The TodoList
      // surfaces planning/planner_failed states via subscribed events.
      planTodo(todo.id).catch(() => {});
      onCreated(todo.id);
      setTitle("");
      onOpenChange(false);
    } catch (e: unknown) {
      setErr(typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add a TODO</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <Input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder='e.g. "Migrate auth to OAuth"'
            onKeyDown={(e) => e.key === "Enter" && submit()}
          />
          {err && <div className="text-xs text-destructive">{err}</div>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={submit} disabled={busy || !title.trim()}>
              {busy ? "Saving..." : "Save and plan"}
            </Button>
          </div>
          <div className="text-[11px] text-muted-foreground">
            FastClaude will use <code>claude -p</code> to suggest subtasks; you'll review them before any session launches.
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
