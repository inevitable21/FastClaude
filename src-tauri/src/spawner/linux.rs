use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};

pub struct LinuxSpawner;

impl Spawner for LinuxSpawner {
    fn spawn(&self, _req: &SpawnRequest) -> AppResult<SpawnResult> {
        Err(AppError::PlatformUnsupported("Linux"))
    }
}
