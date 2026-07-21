//! Memory logger module - captures logs in an in-memory buffer for web viewing

use log::{LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// An in-memory logger that keeps a ring buffer of recent logs and forwards to Syslog if enabled
pub struct MemoryLogger {
    buffer: Mutex<VecDeque<String>>,
}

static LOGGER: OnceLock<MemoryLogger> = OnceLock::new();
static SYSLOG_SENDER: OnceLock<Mutex<Option<SyslogSender>>> = OnceLock::new();

#[cfg(unix)]
use std::os::unix::net::UnixDatagram;

/// Syslog sender forwarding log messages to a local/remote Syslog daemon (RFC 5424)
pub struct SyslogSender {
    pub enabled: bool,
    pub server: String,
    pub protocol: String,
    pub facility: u8,
    pub hostname: String,
    pub app_name: String,
    udp_socket: Option<UdpSocket>,
    tcp_stream: Mutex<Option<TcpStream>>,
    #[cfg(unix)]
    unix_socket: Option<UnixDatagram>,
}

pub fn parse_facility(facility_str: &str) -> u8 {
    match facility_str.trim().to_lowercase().as_str() {
        "kern" | "0" => 0,
        "user" | "1" => 1,
        "mail" | "2" => 2,
        "daemon" | "3" => 3,
        "auth" | "4" => 4,
        "syslog" | "5" => 5,
        "lpr" | "6" => 6,
        "news" | "7" => 7,
        "uucp" | "8" => 8,
        "cron" | "9" => 9,
        "authpriv" | "10" => 10,
        "ftp" | "11" => 11,
        "local0" | "16" => 16,
        "local1" | "17" => 17,
        "local2" | "18" => 18,
        "local3" | "19" => 19,
        "local4" | "20" => 20,
        "local5" | "21" => 21,
        "local6" | "22" => 22,
        "local7" | "23" => 23,
        other => other.parse::<u8>().unwrap_or(1),
    }
}

fn level_to_severity(level: log::Level) -> u8 {
    match level {
        log::Level::Error => 3,
        log::Level::Warn => 4,
        log::Level::Info => 6,
        log::Level::Debug => 7,
        log::Level::Trace => 7,
    }
}

pub fn sys_time_iso8601() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            let millis = dur.subsec_millis();
            let days = secs / 86400;
            let day_secs = secs % 86400;
            let hours = day_secs / 3600;
            let mins = (day_secs % 3600) / 60;
            let s = day_secs % 60;

            let z = days as i64 + 719468;
            let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
            let doe = (z - era * 146097) as u64;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe as i64 + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = if m <= 2 { y + 1 } else { y };

            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                year, m, d, hours, mins, s, millis
            )
        }
        Err(_) => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

impl SyslogSender {
    pub fn new(cfg: &crate::config::SyslogConfig) -> Self {
        let facility = parse_facility(&cfg.facility);
        let hostname = cfg.hostname.clone().unwrap_or_else(|| {
            sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_string())
        });
        let protocol = cfg.protocol.to_lowercase();

        let udp_socket = if protocol == "udp" || (cfg!(not(unix)) && protocol == "unix") {
            let socket = UdpSocket::bind("0.0.0.0:0").ok();
            if let Some(ref s) = socket {
                let _ = s.set_nonblocking(true);
            }
            socket
        } else {
            None
        };

        #[cfg(unix)]
        let unix_socket = if protocol == "unix" {
            UnixDatagram::unbound().ok()
        } else {
            None
        };

        SyslogSender {
            enabled: cfg.enabled,
            server: cfg.server.clone(),
            protocol,
            facility,
            hostname,
            app_name: cfg.app_name.clone(),
            udp_socket,
            tcp_stream: Mutex::new(None),
            #[cfg(unix)]
            unix_socket,
        }
    }

    pub fn send(&self, level: log::Level, target: &str, msg: &str) {
        if !self.enabled {
            return;
        }

        let severity = level_to_severity(level);
        let pri = (self.facility * 8) + severity;
        let timestamp = sys_time_iso8601();
        let pid = std::process::id();

        // RFC 5424 formatted syslog message
        let syslog_msg = format!(
            "<{}>1 {} {} {} {} - - [{}] {}\n",
            pri, timestamp, self.hostname, self.app_name, pid, target, msg
        );

        if self.protocol == "unix" {
            #[cfg(unix)]
            {
                if let Some(ref socket) = self.unix_socket {
                    let path = if self.server.starts_with('/') {
                        &self.server
                    } else {
                        "/dev/log"
                    };
                    let _ = socket.send_to(syslog_msg.as_bytes(), path);
                }
            }
            #[cfg(not(unix))]
            {
                if let Some(ref socket) = self.udp_socket {
                    let _ = socket.send_to(syslog_msg.as_bytes(), &self.server);
                }
            }
        } else if self.protocol == "tcp" {
            if let Ok(mut stream_guard) = self.tcp_stream.lock() {
                let mut reconnect = stream_guard.is_none();
                if let Some(ref mut stream) = *stream_guard {
                    if stream.write_all(syslog_msg.as_bytes()).is_err() {
                        reconnect = true;
                    }
                }
                if reconnect {
                    if let Ok(addrs) = self.server.to_socket_addrs() {
                        if let Some(addr) = addrs.into_iter().next() {
                            if let Ok(mut stream) =
                                TcpStream::connect_timeout(&addr, Duration::from_secs(2))
                            {
                                let _ = stream.write_all(syslog_msg.as_bytes());
                                *stream_guard = Some(stream);
                            } else {
                                *stream_guard = None;
                            }
                        }
                    }
                }
            }
        } else if let Some(ref socket) = self.udp_socket {
            let _ = socket.send_to(syslog_msg.as_bytes(), &self.server);
        }
    }
}

/// Configure or update the active Syslog sender
pub fn configure_syslog(cfg: &crate::config::SyslogConfig) {
    let sender = SyslogSender::new(cfg);
    let cell = SYSLOG_SENDER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cell.lock() {
        *guard = Some(sender);
    }
}

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

            // Forward log to Syslog server if configured
            if let Some(guard) = SYSLOG_SENDER.get() {
                if let Ok(sender_opt) = guard.lock() {
                    if let Some(ref sender) = *sender_opt {
                        sender.send(
                            record.level(),
                            record.target(),
                            &format!("{}", record.args()),
                        );
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;

    #[test]
    fn test_parse_facility() {
        assert_eq!(parse_facility("user"), 1);
        assert_eq!(parse_facility("local0"), 16);
        assert_eq!(parse_facility("local7"), 23);
        assert_eq!(parse_facility("daemon"), 3);
        assert_eq!(parse_facility("16"), 16);
        assert_eq!(parse_facility("invalid"), 1);
    }

    #[test]
    fn test_sys_time_iso8601() {
        let ts = sys_time_iso8601();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 24);
    }

    #[test]
    fn test_syslog_udp_send() {
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();

        let cfg = crate::config::SyslogConfig {
            enabled: true,
            server: addr.to_string(),
            protocol: "udp".to_string(),
            facility: "local0".to_string(),
            hostname: Some("test-host".to_string()),
            app_name: "test-app".to_string(),
        };

        let sender = SyslogSender::new(&cfg);
        sender.send(log::Level::Info, "test_target", "Hello Syslog UDP!");

        let mut buf = [0u8; 1024];
        let (amt, _) = listener
            .recv_from(&mut buf)
            .expect("Failed to receive syslog packet");
        let msg = String::from_utf8_lossy(&buf[..amt]);

        assert!(msg.contains("test-host test-app"));
        assert!(msg.contains("[test_target] Hello Syslog UDP!"));
    }
}
