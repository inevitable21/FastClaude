use super::WindowFocus;
use crate::error::{AppError, AppResult};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

pub struct WinFocus;

impl WindowFocus for WinFocus {
    fn focus(&self, pid: u32, _handle: Option<&str>) -> AppResult<()> {
        let hwnd = find_top_level_window(pid)
            .ok_or_else(|| AppError::Focus(format!("no visible window for pid {pid}")))?;
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

fn find_top_level_window(pid: u32) -> Option<HWND> {
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
