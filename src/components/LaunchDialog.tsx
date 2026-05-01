import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useToast } from "@/hooks/use-toast";
import { launchSession, recentProjects, getConfig } from "@/lib/ipc";
import type { RecentProject, AppConfig } from "@/types";

const MODELS = ["claude-opus-4-7", "claude-sonnet-4-6", "claude-haiku-4-5"];

export function LaunchDialog({
  open,
  onOpenChange,
  onLaunched,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onLaunched: () => void;
}) {
  const { toast } = useToast();
  const [recents, setRecents] = useState<RecentProject[]>([]);
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [projectDir, setProjectDir] = useState("");
  const [model, setModel] = useState<string>(MODELS[0]);
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setErr(null);
    recentProjects(10).then(setRecents).catch(() => setRecents([]));
    getConfig()
      .then((c) => {
        setCfg(c);
        setModel(c.default_model);
      })
      .catch(() => {});
  }, [open]);

  async function submit() {
    if (!projectDir.trim()) {
      setErr("Project folder required");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await launchSession({
        project_dir: projectDir.trim(),
        model,
        prompt: prompt || undefined,
      });
      toast({ title: "Session launched" });
      onLaunched();
      onOpenChange(false);
      setProjectDir("");
      setPrompt("");
    } catch (e: unknown) {
      setErr(typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Launch session</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div>
            <label className="text-xs font-medium">Project folder</label>
            <Input
              value={projectDir}
              onChange={(e) => setProjectDir(e.target.value)}
              placeholder="C:/path/to/project"
            />
            {recents.length > 0 && (
              <div className="mt-2 max-h-40 overflow-auto border rounded">
                {recents.map((r) => (
                  <button
                    key={r.encoded_name}
                    onClick={() => setProjectDir(r.decoded_path)}
                    className="block w-full text-left px-2 py-1 text-xs hover:bg-accent"
                  >
                    {r.decoded_path}
                  </button>
                ))}
              </div>
            )}
          </div>
          <div>
            <label className="text-xs font-medium">Model</label>
            <Select value={model} onValueChange={setModel}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {MODELS.map((m) => (
                  <SelectItem key={m} value={m}>
                    {m}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <label className="text-xs font-medium">Starting prompt (optional)</label>
            <Input
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Implement X..."
            />
          </div>
          {err && <div className="text-xs text-destructive">{err}</div>}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={submit} disabled={busy}>
              {busy ? "Launching..." : "Launch"}
            </Button>
          </div>
          {cfg && (
            <div className="text-[10px] text-muted-foreground">
              terminal: {cfg.terminal_program}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
