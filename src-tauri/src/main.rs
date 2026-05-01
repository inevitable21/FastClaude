#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use fastclaude_lib::{
    commands::{self, AppState},
    config, poller,
    session_registry::Registry,
    spawner, window_focus,
};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&data_dir).ok();

            let cfg_path = data_dir.join("config.json");
            let (cfg, was_created) = config::load(&cfg_path).expect("load config");
            let cfg_arc = Arc::new(Mutex::new(cfg));

            let db_path = data_dir.join("state.db");
            let registry = Arc::new(Registry::open(&db_path).expect("open registry"));

            reconcile_startup(&registry);

            let state = AppState {
                registry: registry.clone(),
                spawner: spawner::default_spawner(),
                focus: window_focus::default_focus(),
                config: cfg_arc.clone(),
                config_path: cfg_path.clone(),
                is_first_run: AtomicBool::new(was_created),
            };
            app.manage(state);

            // Register the configured global hotkey. Failures are non-fatal —
            // the hotkey just won't fire (e.g. invalid binding string).
            let hotkey_str = cfg_arc.lock().unwrap().hotkey.clone();
            let app_for_hk = app.handle().clone();
            match hotkey_str.parse::<tauri_plugin_global_shortcut::Shortcut>() {
                Ok(shortcut) => {
                    let res = app.global_shortcut().on_shortcut(shortcut, move |_app, _sc, event| {
                        if matches!(event.state(), ShortcutState::Pressed) {
                            if let Some(w) = app_for_hk.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                            let _ = app_for_hk.emit("hotkey-fired", ());
                        }
                    });
                    if let Err(e) = res {
                        eprintln!("failed to register hotkey {hotkey_str}: {e}");
                    }
                }
                Err(e) => eprintln!("invalid hotkey '{hotkey_str}' in config: {e}"),
            }

            let app_handle = app.handle().clone();
            let registry_for_poller = registry.clone();
            let cfg_for_poller = cfg_arc.clone();
            tauri::async_runtime::spawn(async move {
                poller::run_loop(
                    registry_for_poller,
                    cfg_for_poller,
                    std::time::Duration::from_secs(2),
                    move |report| {
                        if !report.ended_ids.is_empty() || report.usage_changed {
                            let _ = app_handle.emit("session-changed", &report.ended_ids);
                        }
                    },
                )
                .await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::list_all_sessions,
            commands::launch_session,
            commands::kill_session,
            commands::focus_session,
            commands::recent_projects,
            commands::get_config,
            commands::set_config,
            commands::get_first_run,
            commands::clear_first_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn reconcile_startup(registry: &Registry) {
    use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes();
    let active = registry.list_active().unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    for s in active {
        if sys.process(Pid::from_u32(s.claude_pid as u32)).is_none() {
            let _ = registry.mark_ended(&s.id, now);
        }
    }
}
