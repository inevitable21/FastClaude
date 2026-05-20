import { useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
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
  listProjects,
} from "@/lib/ipc";
import { MODELS } from "@/lib/models";
import {
  EFFORT_OPTIONS,
  PERMISSION_MODE_OPTIONS,
  UNSET,
  fromUnset,
  toUnset,
} from "@/lib/launch-options";
import type { RecentProject, AppConfig, Project } from "@/types";

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
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectDir, setProjectDir] = useState("");
  const [model, setModel] = useState<string>(MODELS[0]);
  const [prompt, setPrompt] = useState("");
  const [effort, setEffort] = useState<string>("");
  const [permissionMode, setPermissionMode] = useState<string>("");
  const [extraArgs, setExtraArgs] = useState<string>("");
  const [autoContinue, setAutoContinue] = useState<boolean>(false);
  const [resumePromptOverride, setResumePromptOverride] = useState<string>("");
  const [showResumePrompt, setShowResumePrompt] = useState<boolean>(false);
  const [preview, setPreview] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [recentIndex, setRecentIndex] = useState<number | null>(null);
  // Session title (free text) + project name (autocomplete over existing
  // projects). projectId is the resolved Project.id when the typed name
  // exactly matches one; null means "let the backend auto-upsert from the
  // folder path".
  const [title, setTitle] = useState<string>("");
  const [projectName, setProjectName] = useState<string>("");
  const [projectId, setProjectId] = useState<string | null>(null);
  const [showProjectSuggestions, setShowProjectSuggestions] = useState(false);
  const recentRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  // Mirror state into refs so the document keydown handler always reads
  // the latest values, regardless of when its closure was created.
  const stateRef = useRef({
    recents,
    recentIndex,
    projectDir,
    model,
    prompt,
    effort,
    permissionMode,
    extraArgs,
  });
  stateRef.current = {
    recents,
    recentIndex,
    projectDir,
    model,
    prompt,
    effort,
    permissionMode,
    extraArgs,
  };

  useEffect(() => {
    if (!open) return;
    setErr(null);
    setRecentIndex(null);
    setTitle("");
    setProjectName("");
    setProjectId(null);
    setShowProjectSuggestions(false);
    recentProjects(10).then(setRecents).catch(() => setRecents([]));
    listProjects().then(setProjects).catch(() => setProjects([]));
    getConfig()
      .then((c) => {
        setCfg(c);
        setModel(c.default_model);
        setEffort(c.default_effort);
        setPermissionMode(c.default_permission_mode);
        setExtraArgs(c.default_extra_args);
        setPrompt(c.default_prompt);
        setAutoContinue(c.default_auto_continue);
        setResumePromptOverride("");
        setShowResumePrompt(false);
      })
      .catch(() => {});
  }, [open]);

  // When the user picks a folder, default the project name to whatever
  // matches that path so the autocomplete shows the right pick. They can
  // still override.
  useEffect(() => {
    if (!projectDir) return;
    const norm = projectDir.replace(/\\/g, "/").toLowerCase().replace(/\/+$/, "");
    const match = projects.find((p) => p.norm_path === norm);
    if (match) {
      setProjectName(match.display_name);
      setProjectId(match.id);
    } else if (!projectName) {
      // Auto-fill from the folder's last segment, but only if the user
      // hasn't typed anything yet.
      const guess = norm.split("/").filter(Boolean).pop() ?? "";
      setProjectName(guess);
      setProjectId(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectDir, projects]);

  // Filter projects by typed prefix; empty input shows all.
  const projectSuggestions = projectName.trim()
    ? projects.filter((p) =>
        p.display_name.toLowerCase().includes(projectName.trim().toLowerCase()),
      )
    : projects;


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
      // Project name autocomplete: only pass an explicit project_id when
      // the typed name resolves to an existing project. Anything else
      // (blank, novel typed name) falls through to the backend's
      // auto-upsert from project_dir.
      const trimmedName = projectName.trim();
      const matched = trimmedName
        ? projects.find(
            (p) => p.display_name.toLowerCase() === trimmedName.toLowerCase(),
          )
        : null;
      const resolvedProjectId = matched?.id ?? projectId ?? undefined;
      await launchSession({
        project_dir: dir,
        model,
        prompt: prompt || undefined,
        effort,
        permission_mode: permissionMode,
        extra_args: extraArgs,
        auto_continue: autoContinue,
        resume_prompt: resumePromptOverride.trim() || undefined,
        title: title.trim() || undefined,
        project: resolvedProjectId,
      });
      toast({ title: "Session launched" });
      onLaunched();
      onOpenChange(false);
      setProjectDir("");
      setPrompt("");
      setTitle("");
      setProjectName("");
      setProjectId(null);
      setRecentIndex(null);
    } catch (e: unknown) {
      setErr(typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  }

  // Capture arrow / enter / esc at the document level while the dialog is
  // open. Reads latest state via stateRef so a fast-typed ↓+Enter doesn't
  // race against React's render cycle.
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      // If a Radix Select popup is open, let it handle its own keys.
      if (target?.closest("[role='listbox']")) return;

      const s = stateRef.current;

      if (s.recents.length > 0 && e.key === "ArrowDown") {
        e.preventDefault();
        const next =
          s.recentIndex === null
            ? 0
            : Math.min(s.recentIndex + 1, s.recents.length - 1);
        recentRefs.current[next]?.scrollIntoView({ block: "nearest" });
        setRecentIndex(next);
        return;
      }
      if (s.recents.length > 0 && e.key === "ArrowUp") {
        e.preventDefault();
        const next =
          s.recentIndex === null
            ? s.recents.length - 1
            : Math.max(s.recentIndex - 1, 0);
        recentRefs.current[next]?.scrollIntoView({ block: "nearest" });
        setRecentIndex(next);
        return;
      }
      if (e.key === "Escape" && s.recentIndex !== null) {
        // Eat the first Esc to clear the highlight; a second Esc still
        // closes the dialog (Radix handles that).
        e.preventDefault();
        e.stopPropagation();
        setRecentIndex(null);
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const picked =
          s.recentIndex !== null ? s.recents[s.recentIndex]?.decoded_path : null;
        const dir = (picked ?? s.projectDir).trim();
        if (dir) {
          if (picked) setProjectDir(picked);
          submit(dir);
        } else {
          setErr("Project folder required");
        }
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[90vh] overflow-y-auto"
        onOpenAutoFocus={(e) => {
          // Radix would otherwise focus its first focusable (often the
          // close X), which swallows our arrow-key onKeyDown.
          e.preventDefault();
          inputRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle>Launch session</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div>
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Project folder</label>
            <Input
              ref={inputRef}
              value={projectDir}
              onChange={(e) => {
                setProjectDir(e.target.value);
                setRecentIndex(null);
              }}
              placeholder="C:/path/to/project"
            />
            {recents.length > 0 && (
              <div className="mt-2 max-h-40 overflow-auto rounded-md border border-border input-fill">
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
                    className={`block w-full text-left px-2 py-1 text-xs font-mono transition-colors hover:bg-foreground/[0.06] ${
                      recentIndex === i
                        ? "bg-gradient-to-r from-[rgba(217,119,87,.25)] to-[rgba(217,119,87,.10)] text-foreground border-l-2 border-accent pl-[6px]"
                        : "border-l-2 border-transparent"
                    }`}
                  >
                    {r.decoded_path}
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div className="relative">
              <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">
                Project
              </label>
              <Input
                value={projectName}
                onChange={(e) => {
                  setProjectName(e.target.value);
                  setProjectId(null);
                  setShowProjectSuggestions(true);
                }}
                onFocus={() => setShowProjectSuggestions(true)}
                onBlur={() =>
                  // Delay so an onClick on a suggestion fires before blur hides it.
                  setTimeout(() => setShowProjectSuggestions(false), 120)
                }
                placeholder="Project name (autocomplete)"
              />
              {showProjectSuggestions && projectSuggestions.length > 0 && (
                <div className="absolute z-10 left-0 right-0 mt-1 max-h-40 overflow-auto rounded-md border border-border input-fill shadow-lg">
                  {projectSuggestions.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      onMouseDown={(e) => {
                        // Use onMouseDown so the click registers before
                        // the input's onBlur fires.
                        e.preventDefault();
                        setProjectName(p.display_name);
                        setProjectId(p.id);
                        setShowProjectSuggestions(false);
                      }}
                      className={`block w-full text-left px-2 py-1 text-xs hover:bg-foreground/[0.06] ${
                        projectId === p.id ? "text-accent" : ""
                      }`}
                    >
                      {p.display_name}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div>
              <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">
                Title (optional)
              </label>
              <Input
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="e.g. Fix login redirect"
              />
            </div>
          </div>
          <div>
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Model</label>
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
              <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">--effort</label>
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
              <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">--permission-mode</label>
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
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Extra args</label>
            <Input
              value={extraArgs}
              onChange={(e) => setExtraArgs(e.target.value)}
              placeholder='--name "MyAgent" --no-session-persistence'
            />
          </div>
          <div>
            <label className="text-[10px] uppercase tracking-[0.10em] text-muted-foreground">Starting prompt (optional)</label>
            <Textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Implement X..."
              className="font-sans"
            />
          </div>
          <div className="rounded-md border border-border p-2 space-y-2">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                className="h-4 w-4 accent-accent"
                checked={autoContinue}
                onChange={(e) => setAutoContinue(e.target.checked)}
              />
              <span className="text-sm">Auto-continue when the 5-hour limit resets</span>
            </label>
            {autoContinue && (
              <>
                <button
                  type="button"
                  onClick={() => setShowResumePrompt((v) => !v)}
                  className="text-[11px] text-accent hover:underline"
                >
                  {showResumePrompt ? "Use default resume prompt" : "Override resume prompt"}
                </button>
                {showResumePrompt && (
                  <Textarea
                    value={resumePromptOverride}
                    onChange={(e) => setResumePromptOverride(e.target.value)}
                    placeholder={cfg?.default_resume_prompt ?? "continue"}
                    className="font-sans"
                  />
                )}
              </>
            )}
          </div>
          {preview && (
            <div className="text-[11px] font-mono input-fill border border-border rounded-md p-2 break-all">
              <span className="text-accent">Will run:</span> {preview}
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
            <div className="text-[10px] font-mono text-muted-foreground">
              terminal: {cfg.terminal_program}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
