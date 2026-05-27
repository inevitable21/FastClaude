import { useCallback, useEffect, useState } from "react";
import { Pin, PinOff, Eye, EyeOff, Pencil, Star } from "lucide-react";
import {
  listProjects,
  listHiddenProjects,
  setProjectName,
  setProjectPinned,
  setProjectHidden,
  setSessionProject,
  upsertProject,
  onProjectChanged,
} from "@/lib/ipc";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useToast } from "@/hooks/use-toast";
import type { Project } from "@/types";
import { DRAG_MIME } from "./SessionRow";

interface Props {
  selectedId: string | null; // null = "All sessions"
  onSelect: (id: string | null) => void;
}

export function ProjectSidebar({ selectedId, onSelect }: Props) {
  const { toast } = useToast();
  const [projects, setProjects] = useState<Project[]>([]);
  const [hiddenProjects, setHiddenProjects] = useState<Project[]>([]);
  const [showHidden, setShowHidden] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");
  // id of the project row a session is currently being dragged over.
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

  function hasSessionDrag(e: React.DragEvent): boolean {
    // dataTransfer.types is the only payload visible during dragover —
    // getData() returns "" outside drop for security reasons.
    return Array.from(e.dataTransfer.types).includes(DRAG_MIME);
  }

  async function handleDrop(e: React.DragEvent, project: Project) {
    e.preventDefault();
    setDropTargetId(null);
    const sessionId = e.dataTransfer.getData(DRAG_MIME);
    if (!sessionId) return;
    try {
      await setSessionProject(sessionId, project.id);
      toast({ title: `Moved to ${project.display_name}` });
    } catch (err) {
      toast({
        title: "Couldn't move session",
        description:
          typeof err === "string"
            ? err
            : (err as { message?: string })?.message ?? String(err),
        variant: "destructive",
      });
    }
  }

  const refresh = useCallback(() => {
    listProjects().then(setProjects).catch(() => setProjects([]));
    listHiddenProjects().then(setHiddenProjects).catch(() => setHiddenProjects([]));
  }, []);

  useEffect(() => {
    refresh();
    const unlisten: Array<() => void> = [];
    onProjectChanged(refresh).then((fn) => unlisten.push(fn));
    return () => unlisten.forEach((u) => u());
  }, [refresh]);

  async function pickFolder() {
    // Native folder picker only — no `window.prompt` fallback. The prompt
    // path let users type bare strings like "asdasd" that became permanent
    // project rows whose later TODO launches failed silently because
    // `wt -d asdasd` can't set a real cwd.
    let path: string | null = null;
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      path = typeof picked === "string" ? picked : null;
    } catch (e) {
      toast({
        title: "Folder picker unavailable",
        description:
          typeof e === "string"
            ? e
            : (e as { message?: string })?.message ?? String(e),
        variant: "destructive",
      });
      return;
    }
    if (!path) return;
    try {
      const p = await upsertProject(path);
      onSelect(p.id);
    } catch (err) {
      toast({
        title: "Couldn't add project",
        description:
          typeof err === "string"
            ? err
            : (err as { message?: string })?.message ?? String(err),
        variant: "destructive",
      });
    }
  }

  function renderRow(p: Project, dimmed = false) {
    const selected = p.id === selectedId;
    const dropping = dropTargetId === p.id;
    return (
      <div
        key={p.id}
        className={`group flex items-center justify-between px-2 py-1 text-xs cursor-pointer ${
          selected ? "bg-foreground/10 border-l-2 border-accent pl-[6px]" : "border-l-2 border-transparent"
        } ${dimmed ? "opacity-50" : ""} ${
          dropping ? "ring-1 ring-accent bg-accent/15" : ""
        }`}
        onClick={() => onSelect(p.id)}
        onDragOver={(e) => {
          if (!hasSessionDrag(e)) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          if (dropTargetId !== p.id) setDropTargetId(p.id);
        }}
        onDragLeave={(e) => {
          // Ignore leave events into child elements; only clear when the
          // pointer actually exits the row.
          if (e.currentTarget.contains(e.relatedTarget as Node)) return;
          if (dropTargetId === p.id) setDropTargetId(null);
        }}
        onDrop={(e) => handleDrop(e, p)}
      >
        {editingId === p.id ? (
          <input
            autoFocus
            className="bg-transparent border-b border-accent outline-none flex-1 mr-2 font-mono"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={() => {
              if (draftName.trim()) setProjectName(p.id, draftName.trim());
              setEditingId(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") setEditingId(null);
            }}
          />
        ) : (
          <span className="flex items-center gap-1 truncate">
            {p.pinned && <Star className="h-3 w-3 text-accent" />}
            <span className="truncate">{p.display_name}</span>
          </span>
        )}
        <div className="opacity-0 group-hover:opacity-100 flex items-center gap-1">
          <button onClick={(e) => { e.stopPropagation(); setEditingId(p.id); setDraftName(p.display_name); }}>
            <Pencil className="h-3 w-3" />
          </button>
          <button onClick={(e) => { e.stopPropagation(); setProjectPinned(p.id, !p.pinned); }}>
            {p.pinned ? <PinOff className="h-3 w-3" /> : <Pin className="h-3 w-3" />}
          </button>
          <button onClick={(e) => { e.stopPropagation(); setProjectHidden(p.id, !p.hidden); }}>
            {p.hidden ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
          </button>
        </div>
      </div>
    );
  }

  return (
    <aside className="w-[220px] shrink-0 border-r border-border flex flex-col">
      <div
        className={`px-2 py-1 text-xs cursor-pointer ${
          selectedId === null ? "bg-foreground/10 border-l-2 border-accent pl-[6px]" : "border-l-2 border-transparent"
        }`}
        onClick={() => onSelect(null)}
      >
        <Star className="inline h-3 w-3 mr-1 text-accent" /> All sessions
      </div>
      <div className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground px-2 py-1 mt-2">
        Projects
      </div>
      <div className="flex-1 overflow-y-auto">
        {projects.map((p) => renderRow(p))}
        {hiddenProjects.length > 0 && (
          <button
            className="block w-full text-left px-2 py-1 text-[11px] text-muted-foreground hover:text-foreground"
            onClick={() => setShowHidden((v) => !v)}
          >
            {showHidden ? "▾" : "▸"} Show hidden ({hiddenProjects.length})
          </button>
        )}
        {showHidden && hiddenProjects.map((p) => renderRow(p, true))}
      </div>
      <button
        className="text-xs px-2 py-2 border-t border-border text-muted-foreground hover:text-foreground"
        onClick={pickFolder}
      >
        + Add project
      </button>
    </aside>
  );
}
