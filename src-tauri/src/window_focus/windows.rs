use super::WindowFocus;
use crate::error::{AppError, AppResult};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

pub struct WinFocus;

impl WindowFocus for WinFocus {
    fn focus(&self, pid: u32, _handle: Option<&str>) -> AppResult<()> {
        let hwnd = find_window_for_pid(pid).or_else(|| find_window_for_ancestor(pid));
        let hwnd = hwnd
            .ok_or_else(|| AppError::Focus(format!("no visible window for pid {pid} or any ancestor")))?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            if !SetForegroundWindow(hwnd).as_bool() {
                return Err(AppError::Focus(format!(
                    "SetForegroundWindow failed for hwnd {:?}",
                    hwnd.0
                )));
            }
        }
        Ok(())
    }
}

/// Walk up the parent chain looking for an ancestor that owns a visible
/// top-level window. The leaf process we track (cmd.exe shell, or node.exe
/// running claude) doesn't own a window — its terminal host (e.g.
/// WindowsTerminal.exe) does.
fn find_window_for_ancestor(start_pid: u32) -> Option<HWND> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    let mut current = sys.process(Pid::from_u32(start_pid))?.parent()?;
    for _ in 0..6 {
        let pid = current.as_u32();
        if let Some(hwnd) = find_window_for_pid(pid) {
            return Some(hwnd);
        }
        current = sys.process(current)?.parent()?;
    }
    None
}

fn find_window_for_pid(pid: u32) -> Option<HWND> {
    struct State {
        target: u32,
        found: Option<HWND>,
    }
    extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let st = &mut *(lparam.0 as *mut State);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut wpid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut wpid));
            if wpid == st.target {
                st.found = Some(hwnd);
                return BOOL(0);
            }
            BOOL(1)
        }
    }
    let mut state = State {
        target: pid,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut state as *mut _ as isize));
    }
    state.found
}
