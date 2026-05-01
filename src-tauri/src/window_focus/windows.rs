use super::WindowFocus;
use crate::error::{AppError, AppResult};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

pub struct WinFocus;

impl WindowFocus for WinFocus {
    fn focus(&self, pid: u32, _handle: Option<&str>) -> AppResult<()> {
        let hwnd = find_window_for_pid(pid)
            .or_else(|| find_window_for_ancestor(pid))
            .ok_or_else(|| {
                AppError::Focus(format!(
                    "no visible window for pid {pid} or any ancestor (up to 10 levels)"
                ))
            })?;
        force_to_foreground(hwnd)
    }
}

/// Bring a window to the foreground bypassing Windows' SetForegroundWindow
/// restrictions. Standard pattern: attach our input queue to both the current
/// foreground thread and the target thread, then call SetForegroundWindow.
fn force_to_foreground(hwnd: HWND) -> AppResult<()> {
    unsafe {
        // Restore if minimized so the window is showable.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let current_thread = GetCurrentThreadId();
        let foreground_window = GetForegroundWindow();
        let foreground_thread = if foreground_window.0 != 0 {
            GetWindowThreadProcessId(foreground_window, None)
        } else {
            0
        };
        let target_thread = GetWindowThreadProcessId(hwnd, None);

        // Press-and-release Alt: tricks Windows into thinking the user just
        // pressed a key, which lifts the SetForegroundWindow restriction.
        keybd_event(VK_MENU.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);

        let attached_fg = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let attached_tgt = target_thread != current_thread
            && AttachThreadInput(current_thread, target_thread, true).as_bool();

        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd).as_bool();

        if attached_tgt {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        if attached_fg {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }

        if !ok {
            return Err(AppError::Focus(format!(
                "SetForegroundWindow failed for hwnd {:?} (Windows may have denied the foreground change)",
                hwnd.0
            )));
        }
    }
    Ok(())
}

/// Walk up the parent chain looking for an ancestor that owns a visible
/// top-level window. The leaf process we track (cmd.exe shell or node.exe
/// running claude) doesn't own a window — only the terminal host does
/// (e.g. WindowsTerminal.exe). Tree typically:
/// WindowsTerminal.exe → OpenConsole.exe → cmd.exe → node.exe
fn find_window_for_ancestor(start_pid: u32) -> Option<HWND> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    let mut current = sys.process(Pid::from_u32(start_pid))?.parent()?;
    for _ in 0..10 {
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
