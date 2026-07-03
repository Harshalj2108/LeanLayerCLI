use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;

use crate::telemetry::errors::TelemetryError;

pub fn init(log_dir: &Path) -> Result<WorkerGuard, TelemetryError> {
    std::fs::create_dir_all(log_dir)?;
    let file_appender = tracing_appender::rolling::daily(log_dir, "leanlayer.jsonl");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(non_blocking)
            .with_target(true)
            .with_thread_ids(true),
    );

    tracing::subscriber::set_global_default(subscriber).map_err(|_| TelemetryError::TracingInit)?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::init;

    #[test]
    fn logger_writes_json_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let guard = init(dir.path()).expect("init logger");
        tracing::info!(answer = 42, "logger test");
        drop(guard);

        let entries = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        assert!(!entries.is_empty());
        let contents = std::fs::read_to_string(entries[0].path()).expect("read log");
        assert!(contents.trim_start().starts_with('{'));
    }
}
