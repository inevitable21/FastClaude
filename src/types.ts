export type SessionStatus = "running" | "idle" | "ended";

export interface Session {
  id: string;
  project_dir: string;
  model: string;
  claude_pid: number;
  terminal_pid: number;
  terminal_window_handle: string | null;
  started_at: number;
  ended_at: number | null;
  jsonl_path: string | null;
  jsonl_offset: number;
  status: SessionStatus;
  last_activity_at: number;
  tokens_in: number;
  tokens_out: number;
  tokens_cache_read: number;
  tokens_cache_write: number;
  auto_continue: boolean;
  resume_prompt: string | null;
  next_resume_at: number | null;
  resume_count: number;
  resume_cap: number;
  resumed_into: string | null;
  resume_failures: number;
  subtask_id: string | null;
}

export interface RecentProject {
  decoded_path: string;
  encoded_name: string;
  mtime: number;
  last_launched_at: number | null;
}

export type LaunchMode = "window" | "minimized" | "hidden";

export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
  default_prompt: string;
  launch_on_login: boolean;
  launch_mode: LaunchMode;
  default_auto_continue: boolean;
  default_resume_prompt: string;
  default_resume_cap: number;
}

export interface LaunchInput {
  project_dir: string;
  model?: string;
  prompt?: string;
  resume?: string;
  effort?: string;
  permission_mode?: string;
  extra_args?: string;
  auto_continue?: boolean;
  resume_prompt?: string;
  subtask_id?: string;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface Project {
  id: string;
  norm_path: string;
  display_name: string;
  pinned: boolean;
  hidden: boolean;
  created_at: number;
}

export type TodoState = "pending" | "ongoing" | "finished";
export type PlannerStatus = "idle" | "planning" | "planned" | "planner_failed";

export interface Todo {
  id: string;
  project_id: string;
  title: string;
  state: TodoState;
  planner_status: PlannerStatus;
  planner_error: string | null;
  auto_suggest_done_at: number | null;
  created_at: number;
  completed_at: number | null;
}

export type SubtaskOrigin = "planner" | "manual";

export interface Subtask {
  id: string;
  todo_id: string;
  ord: number;
  text: string;
  session_id: string | null;
  origin: SubtaskOrigin;
  created_at: number;
}
