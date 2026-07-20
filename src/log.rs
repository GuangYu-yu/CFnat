use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use parking_lot::RwLock;
use serde::Serialize;

const MAX_LOG_ENTRIES: usize = 500;

static START_TIME: RwLock<Option<Instant>> = RwLock::new(None);

fn format_time() -> String {
    let mut guard = START_TIME.write();
    let start = guard.get_or_insert_with(Instant::now);
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}

pub fn reset_start_time() {
    *START_TIME.write() = Some(Instant::now());
}

#[derive(Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

pub struct LogBuffer {
    logs: RwLock<VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            logs: RwLock::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)),
        }
    }

    pub fn push(&self, level: &str, message: &str) {
        let entry = LogEntry {
            timestamp: format_time(),
            level: level.to_string(),
            message: message.to_string(),
        };
        
        let mut logs = self.logs.write();
        if logs.len() >= MAX_LOG_ENTRIES {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    pub fn get_all(&self) -> Vec<LogEntry> {
        self.logs.read().iter().cloned().collect()
    }

    pub fn get_recent(&self, count: usize) -> Vec<LogEntry> {
        let logs = self.logs.read();
        let start = if logs.len() > count { logs.len() - count } else { 0 };
        logs.iter().skip(start).cloned().collect()
    }

    pub fn clear(&self) {
        self.logs.write().clear();
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

static LOG_BUFFER: std::sync::OnceLock<Arc<LogBuffer>> = std::sync::OnceLock::new();

pub fn get_log_buffer() -> Arc<LogBuffer> {
    LOG_BUFFER.get_or_init(|| Arc::new(LogBuffer::new())).clone()
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::log::push_log("INFO", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::log::push_log("WARN", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::log::push_log("ERROR", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::log::push_log("DEBUG", &format!($($arg)*));
    };
}

pub fn push_log(level: &str, message: &str) {
    get_log_buffer().push(level, message);
    
    match level {
        "INFO" => println!("{}", message),
        "WARN" => println!("[警告] {}", message),
        "ERROR" => eprintln!("[错误] {}", message),
        "DEBUG" => {}
        _ => println!("{}", message),
    }
}