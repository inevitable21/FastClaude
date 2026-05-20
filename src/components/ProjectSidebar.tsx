import { useCallback, useEffect, useState } from "react";
import { Pin, PinOff, Eye, EyeOff, Pencil, Star } from "lucide-react";
import {
  listProjects,
  listHiddenProjects,
  setProjectName,
  setProjectPinned,
  setProjectHidden,
  upsertProject,
  onProjectChanged,
} from "@/lib/ipc";
import { open as openDialog } from "@tauri-apps/plugin-dialog"; /* see note below */
import type { Project } from "@/types";

/* NOTE: `@tauri-apps/plugin-dialog` is not currently a dependency. If the
   import errors at build time, replace it with a simple text prompt for v1:
     const path = window.prompt("Project folder path");
   and revisit adding the dialog plugin in a follow-up. */

interface Props {
  selectedId: string | null; // null = "All sessions"
  onSelect: (id: string | null) => void;
}

export function ProjectSidebar({ selectedId, onSelect }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [hiddenProjects, setHiddenProjects] = useState<Project[]>([]);
  const [showHidden, setShowHidden] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");

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
    let path: string | null = null;
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      path = typeof picked === "string" ? picked : null;
    } catch {
      path = window.prompt("Project folder path");
    }
    if (path) {
      const p = await upsertProject(path);
      onSelect(p.id);
    }
  }

  function renderRow(p: Project, dimmed = false) {
    const selected = p.id === selectedId;
    return (
      <div
        key={p.id}
        className={`group flex items-center justify-between px-2 py-1 text-xs cursor-pointer ${
          selected ? "bg-foreground/10 border-l-2 border-accent pl-[6px]" : "border-l-2 border-transparent"
        } ${dimmed ? "opacity-50" : ""}`}
        onClick={() => onSelect(p.id)}
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
