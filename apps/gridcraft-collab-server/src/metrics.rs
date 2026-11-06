//! Prometheus counters and gauges (`/metrics`, RFC §9).
//!
//! Nothing here carries content: no cell values, no tokens, no participant names — only counts
//! and rejection codes.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counters of one process. Gauges are read from the state under its lock when the page is
/// rendered.
#[derive(Default)]
pub struct Metrics {
    pub connections: AtomicU64,
    pub ops: AtomicU64,
    pub applied: AtomicU64,
    pub commits: AtomicU64,
    pub dropped: AtomicU64,
    pub auth_failures: AtomicU64,
    /// Rejections by wire code (`forbidden`, `stale`, …).
    rejections: Mutex<BTreeMap<String, u64>>,
}

impl Metrics {
    pub fn rejection(&self, code: &str) {
        if let Ok(mut codes) = self.rejections.lock() {
            *codes.entry(code.to_string()).or_insert(0) += 1;
        }
    }

    /// `sessions` and `participants` are gauges of the current state, passed in by the caller.
    pub fn render(&self, sessions: u64, participants: u64, leases: u64, version: &str) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("# HELP gridcraft_collab_info Build information.\n");
        out.push_str("# TYPE gridcraft_collab_info gauge\n");
        out.push_str(&format!("gridcraft_collab_info{{version=\"{version}\"}} 1\n"));
        gauge(&mut out, "gridcraft_collab_sessions_open", "Open documents.", sessions);
        gauge(&mut out, "gridcraft_collab_participants", "Participants of all open documents.", participants);
        gauge(&mut out, "gridcraft_collab_leases", "Live file leases.", leases);
        gauge(&mut out, "gridcraft_collab_connections", "Open WebSocket connections.", self.connections.load(Ordering::Relaxed));
        counter(&mut out, "gridcraft_collab_ops_total", "Operations received.");
        out.push_str(&format!("gridcraft_collab_ops_total {}\n", self.ops.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_applied_total", "Operations applied and broadcast.");
        out.push_str(&format!("gridcraft_collab_applied_total {}\n", self.applied.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_commits_total", "Commits served.");
        out.push_str(&format!("gridcraft_collab_commits_total {}\n", self.commits.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_auth_failures_total", "Connections refused before the handshake.");
        out.push_str(&format!("gridcraft_collab_auth_failures_total {}\n", self.auth_failures.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_dropped_frames_total", "Frames dropped for a slow connection.");
        out.push_str(&format!("gridcraft_collab_dropped_frames_total {}\n", self.dropped.load(Ordering::Relaxed)));
        out.push_str("# HELP gridcraft_collab_rejections_total Operations refused, by code.\n");
        out.push_str("# TYPE gridcraft_collab_rejections_total counter\n");
        if let Ok(codes) = self.rejections.lock() {
            for (code, count) in codes.iter() {
                out.push_str(&format!("gridcraft_collab_rejections_total{{code=\"{code}\"}} {count}\n"));
            }
        }
        out
    }
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n"));
}

fn counter(out: &mut String, name: &str, help: &str) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} counter\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_codes_are_labelled_and_counted() {
        let metrics = Metrics::default();
        metrics.rejection("stale");
        metrics.rejection("stale");
        metrics.rejection("forbidden");
        let page = metrics.render(1, 2, 1, "0.16.0");
        assert!(page.contains("gridcraft_collab_sessions_open 1"));
        assert!(page.contains("gridcraft_collab_rejections_total{code=\"stale\"} 2"));
        assert!(page.contains("gridcraft_collab_rejections_total{code=\"forbidden\"} 1"));
    }
}
