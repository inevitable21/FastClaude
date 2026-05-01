import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Session, RecentProject, AppConfig } from "@/types";

export async function listSessions(): Promise<Session[]> {
  return invoke<Session[]>("list_sessions");
}

export async function launchSession(input: {
  project_dir: string;
  model?: string;
  prompt?: string;
}): Promise<Session> {
  return invoke<Session>("launch_session", { input });
}

export async function killSession(id: string): Promise<void> {
  return invoke<void>("kill_session", { id });
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
