//! Process-global log control for the upstream Verovio engine.
//!
//! Verovio's log threshold lives in a namespace-level `vrv::logLevel`
//! variable — not per-toolkit. The function in this module is therefore
//! **process-wide** and gates access through a private `Mutex` so concurrent
//! callers can't race on the FFI boundary.

use std::sync::Mutex;

use verovio_sys::ffi;

/// Verbosity threshold for Verovio's internal log channel.
///
/// Mirrors the upstream `LogLevel` enum at `include/vrv/toolkitdef.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    /// Suppress all log output. Recommended for embedders that don't want
    /// Verovio writing to stdout.
    Off,
    /// Only errors.
    Error,
    /// Errors and warnings (Verovio's default).
    Warning,
    /// Errors, warnings, and informational messages.
    Info,
    /// Everything, including debug traces.
    Debug,
}

impl LogLevel {
    fn as_c_int(self) -> i32 {
        match self {
            LogLevel::Off => 0,
            LogLevel::Error => 1,
            LogLevel::Warning => 2,
            LogLevel::Info => 3,
            LogLevel::Debug => 4,
        }
    }
}

/// Set the Verovio log threshold globally for this process.
///
/// Verovio's log state is namespace-global (see `vrv::logLevel`), not
/// per-toolkit, so this call is **process-wide** — every existing and
/// future `Toolkit` in the process is affected.
///
/// Internally serialized with a mutex so concurrent threads can call this
/// without racing. The mutex is held only for the duration of the upstream
/// `EnableLog` call.
pub fn set_log_level(level: LogLevel) {
    static LOG_MUTEX: Mutex<()> = Mutex::new(());
    let _guard = LOG_MUTEX
        .lock()
        .expect("verovio log-control mutex poisoned");
    ffi::enable_log(level.as_c_int());
}
