use crate::error::AppResult;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

pub trait WindowFocus: Send + Sync {
    /// Bring the window owned by `pid` (or `handle` if provided) to the foreground.
    fn focus(&self, pid: u32, handle: Option<&str>) -> AppResult<()>;
}

pub fn default_focus() -> Box<dyn WindowFocus> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WinFocus)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacFocus)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxFocus)
    }
}
