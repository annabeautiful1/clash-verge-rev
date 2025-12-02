use std::{str::FromStr as _, sync::Arc};

use anyhow::{Result, bail};
use clash_verge_logger::AsyncLogger;
#[cfg(not(feature = "tauri-dev"))]
use clash_verge_logging::NoModuleFilter;
use flexi_logger::{
    Cleanup, Criterion, FileSpec, LogSpecBuilder, LogSpecification, LoggerHandle,
    writers::{FileLogWriter, FileLogWriterBuilder},
};
use log::LevelFilter;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

// TODO: remove it
pub static CLASH_LOGGER: Lazy<Arc<AsyncLogger>> = Lazy::new(|| Arc::new(AsyncLogger::new()));

use crate::{config::Config, singleton, utils::dirs};

#[derive(Default)]
pub struct Logger {
    handle: Arc<Mutex<Option<LoggerHandle>>>,
}

singleton!(Logger, LOGGER);

// TODO: sidecar/service file log writer
impl Logger {
    fn new() -> Self {
        Self::default()
    }

    #[cfg(not(feature = "tauri-dev"))]
    pub async fn init(&self) -> Result<()> {
        let (log_level, log_max_size, log_max_count) = {
            let verge_guard = Config::verge().await;
            let verge = verge_guard.data_arc();
            (
                verge.get_log_level(),
                verge.app_log_max_size.unwrap_or(128),
                verge.app_log_max_count.unwrap_or(8),
            )
        };
        let log_level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|v| log::LevelFilter::from_str(&v).ok())
            .unwrap_or(log_level);
        let spec = Self::generate_log_spec(log_level);
        let log_dir = dirs::app_logs_dir()?;
        let logger = flexi_logger::Logger::with(spec)
            .log_to_file(FileSpec::default().directory(log_dir).basename(""))
            .duplicate_to_stdout(log_level.into())
            .format(clash_verge_logger::console_format)
            .format_for_files(clash_verge_logger::file_format_with_level)
            .rotate(
                Criterion::Size(log_max_size * 1024),
                flexi_logger::Naming::TimestampsCustomFormat {
                    current_infix: Some("latest"),
                    format: "%Y-%m-%d_%H-%M-%S",
                },
                Cleanup::KeepLogFiles(log_max_count),
            );

        let mut filter_modules = vec!["wry", "tokio_tungstenite", "tungstenite"];
        #[cfg(not(feature = "tracing"))]
        filter_modules.push("tauri");
        #[cfg(feature = "tracing")]
        filter_modules.extend(["tauri_plugin_mihomo", "kode_bridge"]);

        let logger = logger.filter(Box::new(NoModuleFilter(filter_modules)));

        let handle = logger.start()?;
        *self.handle.lock() = Some(handle);

        Ok(())
    }

    fn generate_log_spec(log_level: LevelFilter) -> LogSpecification {
        let mut spec = LogSpecBuilder::new();
        let log_level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|v| log::LevelFilter::from_str(&v).ok())
            .unwrap_or(log_level);
        spec.default(log_level);
        #[cfg(feature = "tracing")]
        spec.module("tauri", log::LevelFilter::Debug)
            .module("wry", log::LevelFilter::Off)
            .module("tauri_plugin_mihomo", log::LevelFilter::Off);
        spec.build()
    }

    fn generate_file_log_writer(
        log_max_size: u64,
        log_max_count: usize,
    ) -> Result<FileLogWriterBuilder> {
        let log_dir = dirs::app_logs_dir()?;
        let flwb = FileLogWriter::builder(FileSpec::default().directory(log_dir).basename(""))
            .rotate(
                Criterion::Size(log_max_size * 1024),
                flexi_logger::Naming::TimestampsCustomFormat {
                    current_infix: Some("latest"),
                    format: "%Y-%m-%d_%H-%M-%S",
                },
                Cleanup::KeepLogFiles(log_max_count),
            );
        Ok(flwb)
    }

    pub async fn refresh_log_level(&self) -> Result<()> {
        println!("refresh log level");
        let verge = Config::verge().await;
        let log_level = verge.latest_arc().get_log_level();
        if let Some(handle) = self.handle.lock().as_mut() {
            let log_spec = Self::generate_log_spec(log_level);
            handle.set_new_spec(log_spec);
            handle.adapt_duplication_to_stdout(log_level.into())?;
        } else {
            bail!("failed to get logger handle, make sure it init");
        };
        Ok(())
    }

    pub async fn refresh_log_file(&self) -> Result<()> {
        println!("refresh log file");
        let verge = Config::verge().await;
        let log_max_size = verge.latest_arc().app_log_max_size.unwrap_or(128);
        let log_max_count = verge.latest_arc().app_log_max_count.unwrap_or(8);
        if let Some(handle) = self.handle.lock().as_ref() {
            let log_file_writer = Self::generate_file_log_writer(log_max_size, log_max_count)?;
            handle.reset_flw(&log_file_writer)?;
        } else {
            bail!("failed to get logger handle, make sure it init");
        };
        Ok(())
    }
}
