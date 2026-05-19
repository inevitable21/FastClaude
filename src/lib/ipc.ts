import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Session,
  RecentProject,
  AppConfig,
  UpdateInfo,
  LaunchInput,
  Project,
  Todo,
  Subtask,
} from "@/types";

export async function listSessions(): Promise<Session[]> {
  return invoke<Session[]>("list_sessions");
}

export async function listAllSessions(): Promise<Session[]> {
  return invoke<Session[]>("list_all_sessions");
}

export async function launchSession(input: LaunchInput): Promise<Session> {
  return invoke<Session>("launch_session", { input });
}

export async function previewLaunchCommand(input: LaunchInput): Promise<string> {
  return invoke<string>("preview_launch_command", { input });
}

export async function killSession(id: string): Promise<void> {
  return invoke<void>("kill_session", { id });
}

export async function deleteSession(id: string): Promise<void> {
  return invoke<void>("delete_session", { id });
}

export async function deleteSessions(ids: string[]): Promise<number> {
  return invoke<number>("delete_sessions", { ids });
}

export async function clearEndedSessions(): Promise<number> {
  return invoke<number>("clear_ended_sessions");
}

export async function focusSession(id: string): Promise<void> {
  return invoke<void>("focus_session", { id });
}

export async function recentProjects(limit = 10): Promise<RecentProject[]> {
  return invoke<RecentProject[]>("recent_projects", { limit });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

export async function setConfig(cfg: AppConfig): Promise<void> {
  return invoke<void>("set_config", { cfg });
}

export async function onSessionChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("session-changed", () => handler());
}

export async function onHotkeyFired(handler: () => void): Promise<UnlistenFn> {
  return listen("hotkey-fired", () => handler());
}

export async function getFirstRun(): Promise<boolean> {
  return invoke<boolean>("get_first_run");
}

export async function clearFirstRun(): Promise<void> {
  return invoke<void>("clear_first_run");
}

export async function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

export async function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}

export async function setAutoContinue(id: string, on: boolean): Promise<void> {
  return invoke<void>("set_auto_continue", { id, on });
}

export async function setResumePrompt(id: string, prompt: string | null): Promise<void> {
  return invoke<void>("set_resume_prompt", { id, prompt });
}

export async function onAutoContinueFired(handler: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("auto-continue-fired", (e) => handler(e.payload));
}

export async function onAutoContinueFailed(
  handler: (payload: { id: string; error: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: string; error: string }>("auto-continue-failed", (e) => handler(e.payload));
}

export async function onAutoContinueGaveUp(
  handler: (id: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("auto-continue-gave-up", (e) => handler(e.payload));
}

// Projects
export async function listProjects(): Promise<Project[]> {
  return invoke<Project[]>("list_projects");
}
export async function listHiddenProjects(): Promise<Project[]> {
  return invoke<Project[]>("list_hidden_projects");
}
export async function upsertProject(path: string): Promise<Project> {
  return invoke<Project>("upsert_project", { path });
}
export async function setProjectName(id: string, name: string): Promise<void> {
  return invoke<void>("set_project_name", { id, name });
}
export async function setProjectPinned(id: string, on: boolean): Promise<void> {
  return invoke<void>("set_project_pinned", { id, on });
}
export async function setProjectHidden(id: string, on: boolean): Promise<void> {
  return invoke<void>("set_project_hidden", { id, on });
}
export async function deleteProject(id: string): Promise<void> {
  return invoke<void>("delete_project", { id });
}

// Todos
export async function listTodos(projectId: string): Promise<Todo[]> {
  return invoke<Todo[]>("list_todos", { projectId });
}
export async function createTodo(projectId: string, title: string): Promise<Todo> {
  return invoke<Todo>("create_todo", { projectId, title });
}
export async function planTodo(todoId: string): Promise<void> {
  return invoke<void>("plan_todo", { todoId });
}
export async function deleteTodo(id: string, killRunningSessions: boolean): Promise<void> {
  return invoke<void>("delete_todo", { id, killRunningSessions });
}
export async function markTodoFinished(id: string): Promise<void> {
  return invoke<void>("mark_todo_finished", { id });
}
export async function dismissAutoSuggest(id: string): Promise<void> {
  return invoke<void>("dismiss_auto_suggest", { id });
}

// Subtasks
export async function listSubtasks(todoId: string): Promise<Subtask[]> {
  return invoke<Subtask[]>("list_subtasks", { todoId });
}
export async function addManualSubtask(todoId: string, text: string): Promise<Subtask> {
  return invoke<Subtask>("add_manual_subtask", { todoId, text });
}
export async function editSubtask(id: string, text: string): Promise<void> {
  return invoke<void>("edit_subtask", { id, text });
}
export async function deleteSubtask(id: string): Promise<void> {
  return invoke<void>("delete_subtask", { id });
}
export async function reorderSubtasks(todoId: string, orderedIds: string[]): Promise<void> {
  return invoke<void>("reorder_subtasks", { todoId, orderedIds });
}
export async function launchSubtask(subtaskId: string): Promise<Session> {
  return invoke<Session>("launch_subtask", { subtaskId });
}
export async function launchAllSubtasks(todoId: string): Promise<Session[]> {
  return invoke<Session[]>("launch_all_subtasks", { todoId });
}

// Events
export async function onProjectChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("project-changed", () => handler());
}
export async function onTodoChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("todo-changed", () => handler());
}
