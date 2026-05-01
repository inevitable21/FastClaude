import { useEffect, useRef, useState } from "react";
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
import {
  launchSession,
  recentProjects,
  getConfig,
  previewLaunchCommand,
} from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import {
  EFFORT_OPTIONS,
  PERMISSION_MODE_OPTIONS,
  UNSET,
  fromUnset,
  toUnset,
} from "@/lib/launch-options";
import type { RecentProject, AppConfig } from "@/types";

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
  const [effort, setEffort] = useState<string>("");
  const [permissionMode, setPermissionMode] = useState<string>("");
  const [extraArgs, setExtraArgs] = useState<string>("");
  const [preview, setPreview] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [recentIndex, setRecentIndex] = useState<number | null>(null);
  const recentRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    if (!open) return;
    setErr(null);
    recentProjects(10).then(setRecents).catch(() => setRecents([]));
    getConfig()
      .then((c) => {
        setCfg(c);
        setModel(c.default_model);
        setEffort(c.default_effort);
        setPermissionMode(c.default_permission_mode);
        setExtraArgs(c.default_extra_args);
      })
      .catch(() => {});
  }, [open]);

  // Live preview — backend builds the exact command so the preview matches reality.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    previewLaunchCommand({
      project_dir: projectDir.trim() || ".",
      model,
      prompt: prompt || undefined,
      effort,
      permission_mode: permissionMode,
      extra_args: extraArgs,
    })
      .then((cmd) => {
        if (!cancelled) setPreview(cmd);
      })
      .catch(() => {
        if (!cancelled) setPreview("");
      });
    return () => {
      cancelled = true;
    };
  }, [open, projectDir, model, prompt, effort, permissionMode, extraArgs]);

  async function submit(overrideDir?: string) {
    const dir = (overrideDir ?? projectDir).trim();
    if (!dir) {
      setErr("Project folder required");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await launchSession({
        project_dir: dir,
        model,
        prompt: prompt || undefined,
        effort,
        permission_mode: permissionMode,
        extra_args: extraArgs,
      });
      toast({ title: "Session launched" });
      onLaunched();
      onOpenChange(false);
      setProjectDir("");
      setPrompt("");
      setRecentIndex(null);
    } catch (e: unknown) {
      setErr(typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  }

  function onProjectKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (recents.length > 0 && e.key === "ArrowDown") {
      e.preventDefault();
      setRecentIndex((i) => {
        const next = i === null ? 0 : Math.min(i + 1, recents.length - 1);
        recentRefs.current[next]?.scrollIntoView({ block: "nearest" });
        return next;
      });
      return;
    }
    if (recents.length > 0 && e.key === "ArrowUp") {
      e.preventDefault();
      setRecentIndex((i) => {
        const next = i === null ? recents.length - 1 : Math.max(i - 1, 0);
        recentRefs.current[next]?.scrollIntoView({ block: "nearest" });
        return next;
      });
      return;
    }
    if (e.key === "Escape" && recentIndex !== null) {
      e.preventDefault();
      setRecentIndex(null);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (recentIndex !== null && recents[recentIndex]) {
        const dir = recents[recentIndex].decoded_path;
        setProjectDir(dir);
        submit(dir);
      } else {
        submit();
      }
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Launch session</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div>
            <label className="text-xs font-medium">Project folder</label>
            <Input
              autoFocus
              value={projectDir}
              onChange={(e) => {
                setProjectDir(e.target.value);
                setRecentIndex(null);
              }}
              onKeyDown={onProjectKeyDown}
              placeholder="C:/path/to/project"
            />
            {recents.length > 0 && (
              <div className="mt-2 max-h-40 overflow-auto border rounded">
                {recents.map((r, i) => (
                  <button
                    key={r.encoded_name}
                    ref={(el) => {
                      recentRefs.current[i] = el;
                    }}
                    onClick={() => {
                      setProjectDir(r.decoded_path);
                      setRecentIndex(i);
                    }}
                    className={`block w-full text-left px-2 py-1 text-xs hover:bg-accent ${
                      recentIndex === i ? "bg-accent" : ""
                    }`}
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
                  <SelectItem key={m} value={m}>{m}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className="text-xs font-medium">--effort</label>
              <Select value={toUnset(effort)} onValueChange={(v) => setEffort(fromUnset(v))}>
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
            </div>
            <div>
              <label className="text-xs font-medium">--permission-mode</label>
              <Select
                value={toUnset(permissionMode)}
                onValueChange={(v) => setPermissionMode(fromUnset(v))}
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
            </div>
          </div>
          <div>
            <label className="text-xs font-medium">Extra args</label>
            <Input
              value={extraArgs}
              onChange={(e) => setExtraArgs(e.target.value)}
              placeholder='--name "MyAgent" --no-session-persistence'
            />
          </div>
          <div>
            <label className="text-xs font-medium">Starting prompt (optional)</label>
            <Input
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Implement X..."
            />
          </div>
          {preview && (
            <div className="text-[11px] font-mono bg-muted/50 border border-border rounded p-2 break-all">
              <span className="text-muted-foreground">Will run:</span> {preview}
            </div>
          )}
          {err && <div className="text-xs text-destructive">{err}</div>}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={() => submit()} disabled={busy}>
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
