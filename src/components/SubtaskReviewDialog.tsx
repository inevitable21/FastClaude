import { useCallback, useEffect, useState } from "react";
import { GripVertical, Trash2, Rocket, Plus, RotateCw } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  listSubtasks,
  editSubtask,
  deleteSubtask,
  reorderSubtasks,
  addManualSubtask,
  planTodo,
  launchSubtask,
  launchAllSubtasks,
  onTodoChanged,
} from "@/lib/ipc";
import type { Subtask } from "@/types";
import { useToast } from "@/hooks/use-toast";

interface Props {
  todoId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SubtaskReviewDialog({ todoId, open, onOpenChange }: Props) {
  const { toast } = useToast();
  const [subtasks, setSubtasks] = useState<Subtask[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [dragging, setDragging] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!todoId) return;
    listSubtasks(todoId).then((s) => {
      setSubtasks(s);
      setDrafts((prev) => {
        const next = { ...prev };
        for (const sub of s) {
          if (next[sub.id] === undefined) next[sub.id] = sub.text;
        }
        return next;
      });
    }).catch(() => setSubtasks([]));
  }, [todoId]);

  useEffect(() => {
    if (!open) return;
    refresh();
    const u: Array<() => void> = [];
    onTodoChanged(refresh).then((fn) => u.push(fn));
    return () => u.forEach((fn) => fn());
  }, [open, refresh]);

  if (!todoId) return null;
  // A subtask is "live" only while its attached session is still running.
  // A subtask whose session ended (crashed, killed, closed) is eligible to
  // relaunch and is treated like a fresh subtask for editing/reordering/delete.
  const isLive = (s: typeof subtasks[number]) => !!s.session_id && !s.session_ended;
  const anyLive = subtasks.some(isLive);

  async function saveDraft(id: string) {
    const text = drafts[id];
    const original = subtasks.find((s) => s.id === id)?.text;
    if (text && text !== original) await editSubtask(id, text);
  }

  function onDragStart(id: string) { setDragging(id); }
  function onDragOver(e: React.DragEvent) { e.preventDefault(); }
  async function onDrop(targetId: string) {
    if (!dragging || dragging === targetId) return;
    const order = subtasks.map((s) => s.id);
    const from = order.indexOf(dragging);
    const to = order.indexOf(targetId);
    if (from < 0 || to < 0) return;
    order.splice(to, 0, ...order.splice(from, 1));
    await reorderSubtasks(todoId!, order);
    setDragging(null);
  }

  async function rePlan() {
    if (anyLive) {
      toast({ title: "Cannot re-plan", description: "Stop any running subtask sessions first", variant: "destructive" });
      return;
    }
    await planTodo(todoId!);
  }

  async function launchOne(id: string) {
    try { await launchSubtask(id); }
    catch (e) { toast({ title: "Launch failed", description: String(e), variant: "destructive" }); }
  }
  async function launchAll() {
    try {
      const sessions = await launchAllSubtasks(todoId!);
      toast({ title: `Launched ${sessions.length} session${sessions.length === 1 ? "" : "s"}` });
      onOpenChange(false);
    } catch (e) {
      toast({ title: "Launch all failed", description: String(e), variant: "destructive" });
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Review subtasks</DialogTitle>
        </DialogHeader>
        <div className="space-y-2">
          {subtasks.map((s) => {
            const live = isLive(s);
            const wasLaunched = !!s.session_id && !live;
            return (
            <div
              key={s.id}
              className={`flex items-start gap-2 p-2 rounded border ${live ? "bg-foreground/5" : "border-border"}`}
              draggable={!live}
              onDragStart={() => onDragStart(s.id)}
              onDragOver={onDragOver}
              onDrop={() => onDrop(s.id)}
            >
              <GripVertical className={`h-4 w-4 mt-1 ${live ? "opacity-30" : "opacity-70 cursor-grab"}`} />
              <div className="flex-1 space-y-1">
                <Textarea
                  value={drafts[s.id] ?? s.text}
                  onChange={(e) => setDrafts((d) => ({ ...d, [s.id]: e.target.value }))}
                  onBlur={() => saveDraft(s.id)}
                  disabled={live}
                  className="font-sans w-full"
                  rows={2}
                />
                {wasLaunched && (
                  <div className="text-[10px] text-muted-foreground">
                    Previous session ended — click <Rocket className="inline h-2.5 w-2.5" /> to relaunch
                  </div>
                )}
              </div>
              <div className="flex flex-col gap-1">
                <button
                  title={wasLaunched ? "Relaunch this subtask" : "Launch this subtask"}
                  onClick={() => launchOne(s.id)}
                  disabled={live}
                  className="text-xs disabled:opacity-30"
                >
                  <Rocket className="h-4 w-4" />
                </button>
                <button
                  title="Delete"
                  onClick={() => deleteSubtask(s.id)}
                  disabled={live}
                  className="text-xs disabled:opacity-30"
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
            </div>
            );
          })}
          <button
            className="text-xs px-2 py-1 border border-dashed border-border rounded w-full hover:bg-foreground/5"
            onClick={async () => {
              const text = window.prompt("New subtask text");
              if (text?.trim()) await addManualSubtask(todoId!, text.trim());
            }}
          >
            <Plus className="inline h-3 w-3 mr-1" /> Add manual subtask
          </button>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={rePlan} disabled={anyLive}>
              <RotateCw className="h-3 w-3 mr-1" /> Re-plan
            </Button>
            <Button onClick={launchAll} disabled={subtasks.length === 0}>
              Launch all
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
