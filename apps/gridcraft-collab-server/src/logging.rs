//! Process-wide [`log`] output for the server binary.
//!
//! The library logs through `log::*`; without a logger those records are dropped. [`init`] installs
//! one sink on stderr: one line per record, `timestamp level target: message`. The `color` feature
//! of `env_logger` is off, so a log file or a pipe never sees an ANSI escape.
//!
//! The filter comes from `RUST_LOG`. When it is unset, the collaboration crates are at `info` and
//! everything else is at `warn`, so a normal start says what the process does without the noise of
//! the HTTP stack. `RUST_LOG=off` silences the process; `RUST_LOG=debug` turns everything up.
//!
//! Records carry `fileId`, `sessionId`, `actorId`, a version and an error cause — never a token,
//! a signing key, cell values or a path outside `fileId` (`docs/collab/server.md`,
//! `.craft/protocol/BUS.md` §4.2).

use std::io::Write;

use chrono::Utc;
use env_logger::{Builder, Env};

/// Filter used when `RUST_LOG` is absent: collab crates at `info`, the rest at `warn`.
pub const DEFAULT_FILTER: &str = "warn,gridcraft_collab_server=info,gridcraft_collab=info";

/// Installs the logger. Returns `false` when a logger was already installed (tests, embedded
/// callers); the process keeps running with it rather than panicking.
pub fn init() -> bool {
    builder(Env::default().default_filter_or(DEFAULT_FILTER)).try_init().is_ok()
}

/// The sink `init` installs, without installing it: the tests read its filter from here.
fn builder(env: Env) -> Builder {
    let mut builder = Builder::from_env(env);
    builder.format(|buf, record| writeln!(buf, "{}", render(record.level(), record.target(), &record.args().to_string())));
    builder
}

/// One log line: `timestamp level target: message`, newlines collapsed so a record stays one line.
fn render(level: log::Level, target: &str, message: &str) -> String {
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    format!("{ts} {level:<5} {target}: {}", message.replace('\n', " "))
}

#[cfg(test)]
mod tests {
    use log::Log;

    use super::{DEFAULT_FILTER, Env, builder, render};

    fn enabled_with(filter: &str, level: log::Level, target: &str) -> bool {
        builder(Env::new().default_filter_or(filter)).build().enabled(&log::Metadata::builder().level(level).target(target).build())
    }

    #[test]
    fn the_default_filter_keeps_the_collaboration_crates_at_info_and_the_rest_at_warn() {
        assert!(DEFAULT_FILTER.starts_with("warn,"), "{DEFAULT_FILTER}");
        for target in ["gridcraft_collab_server", "gridcraft_collab"] {
            assert!(enabled_with(DEFAULT_FILTER, log::Level::Info, target), "{target} at info");
            assert!(!enabled_with(DEFAULT_FILTER, log::Level::Debug, target), "{target} at debug");
        }
        for target in ["axum", "hyper", "tungstenite", "gridcraft_engine"] {
            assert!(!enabled_with(DEFAULT_FILTER, log::Level::Info, target), "{target} at info");
            assert!(enabled_with(DEFAULT_FILTER, log::Level::Warn, target), "{target} at warn");
        }
    }

    #[test]
    fn the_environment_overrides_the_default_filter() {
        assert!(!enabled_with("off", log::Level::Error, "gridcraft_collab_server"));
        assert!(enabled_with("debug", log::Level::Debug, "axum"));
        assert!(enabled_with("gridcraft_collab=debug", log::Level::Debug, "gridcraft_collab"));
        assert!(!enabled_with("gridcraft_collab=debug", log::Level::Info, "axum"));
    }

    #[test]
    fn a_record_is_one_plain_line_with_the_time_level_and_target() {
        let line = render(log::Level::Info, "gridcraft_collab_server", "session opened fileId=7 session=s-1 fresh=true");
        let (head, tail) = line.split_once(' ').expect("a timestamp and the rest");
        assert!(head.ends_with('Z') && head.contains('T'), "timestamp first: {head}");
        assert_eq!(tail, "INFO  gridcraft_collab_server: session opened fileId=7 session=s-1 fresh=true");
        assert!(!line.contains('\u{1b}'), "no ANSI escapes: {line:?}");
    }

    #[test]
    fn a_multiline_message_is_collapsed_onto_one_line() {
        let line = render(log::Level::Error, "gridcraft_collab_server", "snapshot failed session=s-1: bad\nthing");
        assert!(!line.contains('\n'), "{line:?}");
        assert!(line.ends_with("snapshot failed session=s-1: bad thing"), "{line:?}");
    }
}
