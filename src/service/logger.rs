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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SipTrace {
    pub timestamp: String,
    pub direction: String, // "IN" or "OUT"
    pub account: String,
    pub message: String,
    pub transport: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CallRecord {
    pub id: String,
    pub account: String,
    pub remote_uri: String,
    pub direction: String, // "IN" or "OUT"
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_secs: u64,
    pub state: String, // "Dialing", "Connected", "Completed", "Failed"
    pub dtmf_digits: String,
}

static SIP_TRACES: OnceLock<Mutex<VecDeque<SipTrace>>> = OnceLock::new();
static CALL_HISTORY: OnceLock<Mutex<VecDeque<CallRecord>>> = OnceLock::new();

fn chrono_like_time() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
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
    }
}

pub fn record_sip_trace(direction: &str, account: &str, message: &str, transport: &str) {
    let traces = SIP_TRACES.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut buf) = traces.lock() {
        let timestamp = chrono_like_time();
        buf.push_back(SipTrace {
            timestamp,
            direction: direction.to_string(),
            account: account.to_string(),
            message: message.to_string(),
            transport: transport.to_string(),
        });
        if buf.len() > 100 {
            buf.pop_front();
        }
    }
}

pub fn get_sip_traces() -> Vec<SipTrace> {
    let traces = SIP_TRACES.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(buf) = traces.lock() {
        return buf.iter().cloned().collect();
    }
    vec![]
}

pub fn record_call_start(id: &str, account: &str, remote_uri: &str, direction: &str) {
    let history = CALL_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut buf) = history.lock() {
        buf.retain(|c| c.id != id);
        let start_time = chrono_like_time();
        buf.push_back(CallRecord {
            id: id.to_string(),
            account: account.to_string(),
            remote_uri: remote_uri.to_string(),
            direction: direction.to_string(),
            start_time,
            end_time: None,
            duration_secs: 0,
            state: "Dialing".to_string(),
            dtmf_digits: String::new(),
        });
        if buf.len() > 50 {
            buf.pop_front();
        }
    }
}

pub fn record_call_connect(id: &str) {
    let history = CALL_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut buf) = history.lock() {
        if let Some(call) = buf.iter_mut().find(|c| c.id == id) {
            call.state = "Connected".to_string();
        }
    }
}

pub fn record_call_end(id: &str, state: &str, duration_secs: u64) {
    let history = CALL_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut buf) = history.lock() {
        if let Some(call) = buf.iter_mut().find(|c| c.id == id) {
            call.state = state.to_string();
            call.end_time = Some(chrono_like_time());
            call.duration_secs = duration_secs;
        }
    }
}

pub fn record_call_dtmf(id: &str, digit: &str) {
    let history = CALL_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(mut buf) = history.lock() {
        if let Some(call) = buf.iter_mut().find(|c| c.id == id) {
            call.dtmf_digits.push_str(digit);
        }
    }
}

pub fn get_call_history() -> Vec<CallRecord> {
    let history = CALL_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Ok(buf) = history.lock() {
        return buf.iter().cloned().collect();
    }
    vec![]
}
