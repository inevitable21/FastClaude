import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";

export function EmptyState({
  onLaunch,
  hotkey,
}: {
  onLaunch: () => void;
  hotkey?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center gap-3">
      <button
        onClick={onLaunch}
        aria-label="Launch new session"
        title="Launch new session"
        className="flex h-14 w-14 items-center justify-center rounded-full border border-border bg-accent/10 text-accent transition hover:bg-accent/20 hover:border-accent/50 hover:scale-105 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
      >
        <Plus className="h-6 w-6" />
      </button>
      <h2 className="text-lg font-semibold">No running sessions</h2>
      <p className="text-sm text-muted-foreground">
        Launch one to get started{hotkey ? <>, or hit <kbd className="inline-block rounded-md border border-border bg-foreground/[0.05] px-1.5 py-0.5 font-mono text-[11px]">{hotkey}</kbd> from anywhere.</> : "."}
      </p>
      <Button onClick={onLaunch} className="mt-2">
        <Plus className="h-4 w-4" />
        Launch new session
      </Button>
    </div>
  );
}
