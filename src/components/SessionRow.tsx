import { Button } from "@/components/ui/button";
import type { Session } from "@/types";
import { focusSession, killSession } from "@/lib/ipc";

function elapsed(startedAt: number): string {
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}

export function SessionRow({
  session,
  onChange,
}: {
  session: Session;
  onChange: () => void;
}) {
  const dot =
    session.status === "running"
      ? "bg-emerald-500"
      : session.status === "idle"
      ? "bg-amber-400"
      : "bg-zinc-400";
  const projectName =
    session.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? session.project_dir;

  async function focus() {
    try {
      await focusSession(session.id);
    } catch {
      /* toast in Plan 2 */
    }
    onChange();
  }
  async function kill() {
    try {
      await killSession(session.id);
    } catch {
      /* toast in Plan 2 */
    }
    onChange();
  }

  return (
    <div className="flex items-center gap-3 rounded-lg border border-border p-3">
      <div className={`h-2 w-2 rounded-full ${dot}`} />
      <div className="flex-1 min-w-0">
        <div className="font-semibold text-sm truncate">{projectName}</div>
        <div className="text-xs text-muted-foreground truncate">{session.project_dir}</div>
      </div>
      <div className="text-xs text-muted-foreground">{elapsed(session.started_at)}</div>
      <span className="text-xs px-2 py-0.5 rounded bg-blue-100 text-blue-800">
        {session.model}
      </span>
      <Button size="sm" onClick={focus}>
        Focus
      </Button>
      <Button size="sm" variant="destructive" onClick={kill}>
        Kill
      </Button>
    </div>
  );
}
