import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import type { Session } from "@/types";
import { focusSession, killSession } from "@/lib/ipc";

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
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

export function SessionRow({
  session,
  onChange,
  index = 0,
}: {
  session: Session;
  onChange: () => void;
  index?: number;
}) {
  const { toast } = useToast();

  const dotClass =
    session.status === "running"
      ? "bg-[var(--status-running)] dot-running-glow"
      : session.status === "idle"
      ? "bg-[var(--status-idle)]"
      : "bg-[var(--status-stopped)]";

  const projectName =
    session.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? session.project_dir;

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

  return (
    <div
      className="flex items-center gap-3 rounded-lg glass-panel p-3 transition-colors hover:border-border-strong animate-row-in"
      style={{ animationDelay: `${index * 70}ms` }}
    >
      <div aria-hidden className={`h-2 w-2 rounded-full flex-shrink-0 ${dotClass}`} />
      <div className="flex-1 min-w-0">
        <div className="font-semibold text-sm truncate">{projectName}</div>
        <div className="text-xs text-muted-foreground truncate font-mono">{session.project_dir}</div>
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
      <Button size="sm" variant="ghost" onClick={focus}>
        Focus
      </Button>
      <Button size="sm" variant="destructive" onClick={kill}>
        Kill
      </Button>
    </div>
  );
}
