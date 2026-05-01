import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig, clearFirstRun } from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import type { AppConfig } from "@/types";
import { HotkeyCapture } from "./HotkeyCapture";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8 text-muted-foreground">Loading...</div>;

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
        <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
        {hint && <div className="text-xs text-muted-foreground mt-1.5">{hint}</div>}
      </label>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center text-foreground px-4">
      <div className="max-w-md w-full p-6 space-y-5 glass-panel-strong rounded-xl shadow-2xl">
        <div className="flex items-center gap-2.5">
          <div
            aria-hidden
            className="h-7 w-7 rounded-md shadow-[0_0_14px_rgba(217,119,87,.45)]"
            style={{ background: "linear-gradient(135deg,#E8825E,#C46141)" }}
          />
          <div>
            <div className="text-xl font-bold tracking-tight">Welcome to FastClaude</div>
            <div className="text-sm text-muted-foreground">Three quick choices and you're set.</div>
          </div>
        </div>
        {field(
          "Terminal program",
          draft.terminal_program,
          (v) => setDraft({ ...draft, terminal_program: v }),
          "'auto' picks Windows Terminal if installed, else cmd.exe"
        )}
        <label className="block">
          <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">Default model</div>
          <Select
            value={draft.default_model}
            onValueChange={(v) => setDraft({ ...draft, default_model: v })}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {MODELS.map((m) => (
                <SelectItem key={m} value={m}>{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <label className="block">
          <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">Global hotkey</div>
          <HotkeyCapture
            value={draft.hotkey}
            onChange={(v) => setDraft({ ...draft, hotkey: v })}
          />
          <div className="text-xs text-muted-foreground mt-1.5">Pressed from anywhere to open the launch dialog</div>
        </label>
        <div className="pt-2">
          <Button onClick={getStarted} className="w-full">Get started</Button>
        </div>
      </div>
    </div>
  );
}
