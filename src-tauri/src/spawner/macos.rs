use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};

pub struct MacSpawner;

impl Spawner for MacSpawner {
    fn spawn(&self, _req: &SpawnRequest) -> AppResult<SpawnResult> {
        Err(AppError::Spawn("macOS spawner ships in Plan 2".into()))
    }
}
