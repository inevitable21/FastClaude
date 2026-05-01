import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useToast } from "@/hooks/use-toast";
import { checkForUpdate, installUpdate } from "@/lib/ipc";
import type { UpdateInfo } from "@/types";

export function UpdateBanner() {
  const { toast } = useToast();
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => {
      checkForUpdate().then(setUpdate).catch(() => {});
    }, 5000);
    return () => clearTimeout(t);
  }, []);

  if (!update || dismissed) return null;

  async function install() {
    try {
      await installUpdate();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update failed", description: msg, variant: "destructive" });
    }
  }

  return (
    <div
      className="flex items-center gap-3 px-4 py-2 text-sm border-b border-border-strong"
      style={{
        background:
          "linear-gradient(90deg, rgba(217,119,87,.20), rgba(217,119,87,.06))",
      }}
    >
      <div
        aria-hidden
        className="h-2 w-2 rounded-full bg-accent shadow-[0_0_8px_rgba(244,181,138,.6)]"
      />
      <div className="flex-1">FastClaude {update.version} is available.</div>
      <button
        onClick={install}
        className="inline-flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium border border-border bg-foreground/[0.04] text-foreground hover:bg-foreground/[0.08] transition"
      >
        Restart &amp; install
      </button>
      <button
        onClick={() => setDismissed(true)}
        title="Dismiss"
        aria-label="Dismiss"
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
