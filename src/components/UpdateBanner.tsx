import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useToast } from "@/hooks/use-toast";
import { checkForUpdate, installUpdate } from "@/lib/ipc";
import type { UpdateInfo } from "@/types";

export function UpdateBanner() {
  const { toast } = useToast();
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    const t = setTimeout(() => {
      checkForUpdate().then(setUpdate).catch(() => {});
    }, 5000);
    return () => clearTimeout(t);
  }, []);

  if (!update) return null;

  async function install() {
    try {
      await installUpdate();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update failed", description: msg, variant: "destructive" });
    }
  }

  return (
    <div className="flex items-center gap-3 px-4 py-2 bg-blue-50 text-blue-900 border-b border-blue-200 text-sm">
      <div className="flex-1">FastClaude {update.version} is available.</div>
      <Button size="sm" onClick={install}>Restart to install</Button>
    </div>
  );
}
