import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig } from "@/lib/ipc";
import type { AppConfig } from "@/types";

export function Settings({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8">Loading...</div>;

  async function save() {
    if (!draft) return;
    try {
      await setConfig(draft);
      toast({ title: "Settings saved" });
      onBack();
    } catch (e: unknown) {
      const msg =
        typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Failed to save", description: msg, variant: "destructive" });
    }
  }

  function field(
    label: string,
    value: string,
    onChange: (v: string) => void,
  ) {
    return (
      <label className="block">
        <div className="text-xs font-medium mb-1">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    );
  }

  return (
    <div className="bg-background text-foreground">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
        <button onClick={onBack} className="text-sm hover:underline">
          ← Back
        </button>
        <div className="font-semibold">Settings</div>
      </div>
      <div className="p-4 space-y-4 max-w-xl min-h-[60vh]">
        {field("Terminal program (or 'auto')", draft.terminal_program, (v) =>
          setDraft({ ...draft, terminal_program: v })
        )}
        {field("Default model", draft.default_model, (v) =>
          setDraft({ ...draft, default_model: v })
        )}
        {field("Global hotkey", draft.hotkey, (v) =>
          setDraft({ ...draft, hotkey: v })
        )}
        {field(
          "Idle threshold (seconds)",
          String(draft.idle_threshold_seconds),
          (v) => {
            const n = parseInt(v, 10);
            if (!Number.isNaN(n) && n > 0) {
              setDraft({ ...draft, idle_threshold_seconds: n });
            }
          }
        )}
        <div className="pt-4 flex gap-2 justify-end">
          <Button variant="ghost" onClick={onBack}>
            Cancel
          </Button>
          <Button onClick={save}>Save</Button>
        </div>
        <p className="text-xs text-muted-foreground pt-4">
          Hotkey changes take effect after restart. Per-model pricing is editable
          by hand in <code>%APPDATA%/FastClaude/config.json</code>.
        </p>
      </div>
    </div>
  );
}
