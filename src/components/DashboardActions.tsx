import { Plus, History as HistoryIcon, Settings as SettingsIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

export function DashboardActions({
  onLaunch,
  onOpenHistory,
  onOpenSettings,
}: {
  onLaunch: () => void;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <Button onClick={onLaunch}>
        <Plus className="h-4 w-4" />
        Launch new session
      </Button>
      <button
        onClick={onOpenHistory}
        title="History"
        aria-label="History"
        className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
      >
        <HistoryIcon className="h-4 w-4" />
      </button>
      <button
        onClick={onOpenSettings}
        title="Settings"
        aria-label="Settings"
        className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
      >
        <SettingsIcon className="h-4 w-4" />
      </button>
    </div>
  );
}
