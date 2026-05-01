use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};

pub struct MacSpawner;

impl Spawner for MacSpawner {
    fn spawn(&self, _req: &SpawnRequest) -> AppResult<SpawnResult> {
        Err(AppError::PlatformUnsupported("macOS"))
    }
}
