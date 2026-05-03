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
}

export interface RecentProject {
  decoded_path: string;
  encoded_name: string;
  mtime: number;
  last_launched_at: number | null;
}

export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
}

export interface LaunchInput {
  project_dir: string;
  model?: string;
  prompt?: string;
  resume?: string;
  effort?: string;
  permission_mode?: string;
  extra_args?: string;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}
