import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig, clearFirstRun } from "@/lib/ipc";
import type { AppConfig } from "@/types";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8">Loading...</div>;

  async function getStarted() {
    if (!draft) return;
    try {
      await setConfig(draft);
      await clearFirstRun();
      onDone();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't save", description: msg, variant: "destructive" });
    }
  }

  function field(label: string, value: string, onChange: (v: string) => void, hint?: string) {
    return (
      <label className="block">
        <div className="text-xs font-medium mb-1">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
        {hint && <div className="text-xs text-muted-foreground mt-1">{hint}</div>}
      </label>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background text-foreground">
      <div className="max-w-md w-full p-6 space-y-4 border border-border rounded-lg">
        <div>
          <div className="text-2xl font-bold">Welcome to FastClaude</div>
          <div className="text-sm text-muted-foreground mt-1">
            Three quick choices and you're set.
          </div>
        </div>
        {field(
          "Terminal program",
          draft.terminal_program,
          (v) => setDraft({ ...draft, terminal_program: v }),
          "'auto' picks Windows Terminal if installed, else cmd.exe"
        )}
        {field(
          "Default model",
          draft.default_model,
          (v) => setDraft({ ...draft, default_model: v }),
          "e.g. claude-opus-4-7, claude-sonnet-4-6, claude-haiku-4-5"
        )}
        {field(
          "Global hotkey",
          draft.hotkey,
          (v) => setDraft({ ...draft, hotkey: v }),
          "Pressed from anywhere to open the launch dialog"
        )}
        <div className="pt-2">
          <Button onClick={getStarted} className="w-full">Get started</Button>
        </div>
      </div>
    </div>
  );
}
