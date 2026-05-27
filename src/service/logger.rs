//! Memory logger module - captures logs in an in-memory buffer for web viewing

use log::{LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// An in-memory logger that keeps a ring buffer of recent logs
pub struct MemoryLogger {
    buffer: Mutex<VecDeque<String>>,
}

static LOGGER: OnceLock<MemoryLogger> = OnceLock::new();

impl Log for MemoryLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let time_str = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            {
                Ok(dur) => {
                    let secs = dur.as_secs();
                    format!(
                        "{:02}:{:02}:{:02}",
                        (secs / 3600) % 24,
                        (secs / 60) % 60,
                        secs % 60
                    )
                }
                Err(_) => "00:00:00".to_string(),
            };

            let level_str = match record.level() {
                log::Level::Error => "ERROR",
                log::Level::Warn => "WARN ",
                log::Level::Info => "INFO ",
                log::Level::Debug => "DEBUG",
                log::Level::Trace => "TRACE",
            };

            let log_line = format!(
                "[{}] {} [{}] {}",
                time_str,
                level_str,
                record.target(),
                record.args()
            );

            // Print to standard error so it is visible in the terminal
            eprintln!("{}", log_line);

            // Push to memory buffer
            if let Ok(mut buf) = self.buffer.lock() {
                buf.push_back(log_line);
                if buf.len() > 200 {
                    buf.pop_front();
                }
            }
        }
    }

    fn flush(&self) {}
}

/// Initialize the global memory logger
pub fn init_logger() {
    let logger = LOGGER.get_or_init(|| MemoryLogger {
        buffer: Mutex::new(VecDeque::new()),
    });
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Info);
}

/// Retrieve a copy of the recent log lines from the memory buffer
pub fn get_recent_logs() -> Vec<String> {
    if let Some(logger) = LOGGER.get() {
        if let Ok(buf) = logger.buffer.lock() {
            return buf.iter().cloned().collect();
        }
    }
    vec![]
}
