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
import { getConfig, setConfig, checkForUpdate } from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import {
  EFFORT_OPTIONS,
  PERMISSION_MODE_OPTIONS,
  UNSET,
  fromUnset,
  toUnset,
} from "@/lib/launch-options";
import type { AppConfig } from "@/types";

export function Settings({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8">Loading...</div>;

  async function checkUpdates() {
    try {
      const u = await checkForUpdate();
      if (u) {
        toast({ title: `FastClaude ${u.version} available`, description: "Restart from the banner to install." });
      } else {
        toast({ title: "You're up to date" });
      }
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Update check failed", description: msg, variant: "destructive" });
    }
  }

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
        <label className="block">
          <div className="text-xs font-medium mb-1">Default model</div>
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
          <div className="text-xs font-medium mb-1">Default --effort</div>
          <Select
            value={toUnset(draft.default_effort)}
            onValueChange={(v) =>
              setDraft({ ...draft, default_effort: fromUnset(v) })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={UNSET}>(don't pass)</SelectItem>
              {EFFORT_OPTIONS.map((e) => (
                <SelectItem key={e} value={e}>{e}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <label className="block">
          <div className="text-xs font-medium mb-1">Default --permission-mode</div>
          <Select
            value={toUnset(draft.default_permission_mode)}
            onValueChange={(v) =>
              setDraft({ ...draft, default_permission_mode: fromUnset(v) })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={UNSET}>(don't pass)</SelectItem>
              {PERMISSION_MODE_OPTIONS.map((m) => (
                <SelectItem key={m} value={m}>{m}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <label className="block">
          <div className="text-xs font-medium mb-1">Default extra args (free-form)</div>
          <Input
            value={draft.default_extra_args}
            onChange={(e) =>
              setDraft({ ...draft, default_extra_args: e.target.value })
            }
            placeholder='e.g. --name "MyAgent" --no-session-persistence'
          />
        </label>
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
          <Button variant="ghost" onClick={checkUpdates}>Check for updates</Button>
          <Button variant="ghost" onClick={onBack}>
            Cancel
          </Button>
          <Button onClick={save}>Save</Button>
        </div>
        <p className="text-xs text-muted-foreground pt-4">
          Hotkey changes take effect after restart.
        </p>
      </div>
    </div>
  );
}
