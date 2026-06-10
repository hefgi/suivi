//! Optional structured logging.
//!
//! Off by default. Enabled via `SUIVI_LOG=<filter>` (uses tracing-subscriber's
//! `EnvFilter` syntax — e.g. `info`, `debug`, `suivi=trace,off`). When enabled,
//! emits one JSON event per line to a daily-rotating file under
//! `$XDG_STATE_HOME/suivi/` (or `~/.local/state/suivi/`).
//!
//! The returned `LogGuard` must live for the duration of the process — it
//! holds the non-blocking writer's worker handle, which flushes on drop.

use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};

/// Holds resources that must outlive the program: the non-blocking writer's
/// worker guard. Dropping it flushes pending log events.
pub struct LogGuard {
    _guard: Option<WorkerGuard>,
}

/// XDG state dir for suivi: `$XDG_STATE_HOME/suivi/` or `~/.local/state/suivi/`.
pub fn log_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("suivi")
}

/// Initialise the global tracing subscriber if `SUIVI_LOG` is set and non-empty.
/// Otherwise returns a no-op guard.
pub fn init() -> LogGuard {
    let filter_str = std::env::var("SUIVI_LOG").unwrap_or_default();
    if filter_str.is_empty() || filter_str.eq_ignore_ascii_case("off") {
        return LogGuard { _guard: None };
    }
    let env_filter = match EnvFilter::try_new(&filter_str) {
        Ok(f) => f,
        Err(_) => return LogGuard { _guard: None },
    };

    let dir = log_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return LogGuard { _guard: None };
    }
    let file_appender = tracing_appender::rolling::daily(&dir, "suivi.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .with_target(true)
        .finish();

    // try_set_global avoids panicking when init() is called twice (e.g. in tests).
    let _ = tracing::subscriber::set_global_default(subscriber);

    LogGuard {
        _guard: Some(guard),
    }
}
