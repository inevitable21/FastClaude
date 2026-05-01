use super::WindowFocus;
use crate::error::{AppError, AppResult};

pub struct MacFocus;

impl WindowFocus for MacFocus {
    fn focus(&self, _pid: u32, _handle: Option<&str>) -> AppResult<()> {
        Err(AppError::PlatformUnsupported("macOS"))
    }
}
