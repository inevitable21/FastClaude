use super::WindowFocus;
use crate::error::{AppError, AppResult};

pub struct LinuxFocus;

impl WindowFocus for LinuxFocus {
    fn focus(&self, _pid: u32, _handle: Option<&str>) -> AppResult<()> {
        Err(AppError::PlatformUnsupported("Linux"))
    }
}
