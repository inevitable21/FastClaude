import { useEffect, useState } from "react";
import { Sun, Moon } from "lucide-react";
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
import { HotkeyCapture } from "./HotkeyCapture";

type Theme = "dark" | "light";

function readTheme(): Theme {
  return localStorage.getItem("fastclaude-theme") === "light" ? "light" : "dark";
}

function applyTheme(t: Theme) {
  if (t === "dark") {
    document.documentElement.classList.add("dark");
    localStorage.setItem("fastclaude-theme", "dark");
  } else {
    document.documentElement.classList.remove("dark");
    localStorage.setItem("fastclaude-theme", "light");
  }
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="glass-panel rounded-xl p-4 space-y-3">
      <div className="text-[10px] uppercase tracking-[0.12em] text-accent">{title}</div>
      {children}
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <div className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground mb-1.5">
        {label}
      </div>
      {children}
    </label>
  );
}

export function Settings({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [draft, setDraft] = useState<AppConfig | null>(null);
  const [theme, setTheme] = useState<Theme>(() => readTheme());

  useEffect(() => {
    getConfig().then(setDraft).catch(() => {});
  }, []);

  if (!draft) return <div className="p-8 text-muted-foreground">Loading...</div>;

  async function checkUpdates() {
    try {
      const u = await checkForUpdate();
      if (u) {
        toast({
          title: `FastClaude ${u.version} available`,
          description: "Restart from the banner to install.",
        });
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
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Failed to save", description: msg, variant: "destructive" });
    }
  }

  function toggleTheme() {
    const next: Theme = theme === "dark" ? "light" : "dark";
    applyTheme(next);
    setTheme(next);
  }

  return (
    <div className="text-foreground">
      <div className="p-4 space-y-4 max-w-xl min-h-[60vh]">
        <Section title="Terminal">
          <Field label="Terminal program (or 'auto')">
            <Input
              value={draft.terminal_program}
              onChange={(e) => setDraft({ ...draft, terminal_program: e.target.value })}
            />
          </Field>
        </Section>

        <Section title="Defaults">
          <Field label="Default model">
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
          </Field>
          <Field label="Default --effort">
            <Select
              value={toUnset(draft.default_effort)}
              onValueChange={(v) => setDraft({ ...draft, default_effort: fromUnset(v) })}
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
          </Field>
          <Field label="Default --permission-mode">
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
          </Field>
          <Field label="Default extra args (free-form)">
            <Input
              value={draft.default_extra_args}
              onChange={(e) => setDraft({ ...draft, default_extra_args: e.target.value })}
              placeholder='e.g. --name "MyAgent" --no-session-persistence'
            />
          </Field>
        </Section>

        <Section title="Hotkey">
          <Field label="Global hotkey">
            <HotkeyCapture
              value={draft.hotkey}
              onChange={(v) => setDraft({ ...draft, hotkey: v })}
            />
          </Field>
          <Field label="Idle threshold (seconds)">
            <Input
              value={String(draft.idle_threshold_seconds)}
              onChange={(e) => {
                const n = parseInt(e.target.value, 10);
                if (!Number.isNaN(n) && n > 0) {
                  setDraft({ ...draft, idle_threshold_seconds: n });
                }
              }}
            />
          </Field>
          <p className="text-xs text-muted-foreground">
            Hotkey changes take effect after restart.
          </p>
        </Section>

        <Section title="Theme">
          <div className="flex items-center gap-3">
            <div className="flex-1 text-sm text-muted-foreground">
              Currently: <span className="text-foreground font-medium">{theme === "dark" ? "Dark (Warm Aurora)" : "Light"}</span>
            </div>
            <Button variant="ghost" onClick={toggleTheme}>
              {theme === "dark" ? (
                <>
                  <Sun className="h-4 w-4" /> Switch to light
                </>
              ) : (
                <>
                  <Moon className="h-4 w-4" /> Switch to dark
                </>
              )}
            </Button>
          </div>
        </Section>

        <Section title="Updates">
          <Button variant="ghost" onClick={checkUpdates}>Check for updates</Button>
        </Section>

        <div className="pt-2 flex gap-2 justify-end">
          <Button variant="ghost" onClick={onBack}>Cancel</Button>
          <Button onClick={save}>Save</Button>
        </div>
      </div>
    </div>
  );
}
