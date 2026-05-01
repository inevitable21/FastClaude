import { Plus } from "lucide-react";

export function EmptyState({
  onLaunch,
  hotkey,
}: {
  onLaunch: () => void;
  hotkey?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center gap-3">
      <div className="flex h-14 w-14 items-center justify-center rounded-full border border-border bg-accent/10 text-accent">
        <Plus className="h-6 w-6" />
      </div>
      <h2 className="text-lg font-semibold">No running sessions</h2>
      <p className="text-sm text-muted-foreground">
        Launch one to get started{hotkey ? <>, or hit <kbd className="inline-block rounded-md border border-border bg-foreground/[0.05] px-1.5 py-0.5 font-mono text-[11px]">{hotkey}</kbd> from anywhere.</> : "."}
      </p>
      <button
        onClick={onLaunch}
        className="btn-primary-gradient mt-2 inline-flex items-center gap-1.5 px-4 py-2 rounded-md text-sm font-semibold text-primary-foreground shadow-[0_8px_24px_rgba(217,119,87,.32),inset_0_1px_0_rgba(255,255,255,.18)] hover:brightness-110 transition"
      >
        <Plus className="h-4 w-4" />
        Launch new session
      </button>
    </div>
  );
}
