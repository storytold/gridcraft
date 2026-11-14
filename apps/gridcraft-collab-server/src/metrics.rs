//! Prometheus counters and gauges (`/metrics`, RFC §9).
//!
//! Nothing here carries content: no cell values, no tokens, no participant names — only counts,
//! durations and rejection codes.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Bucket bounds of the `op → applied` histogram, in seconds as Prometheus labels them. The last
/// slot of [`Histogram::buckets`] is the overflow (`+Inf`).
const LATENCY_BUCKETS: [(&str, u64); 8] = [
    ("0.001", 1_000),
    ("0.005", 5_000),
    ("0.010", 10_000),
    ("0.025", 25_000),
    ("0.050", 50_000),
    ("0.100", 100_000),
    ("0.500", 500_000),
    ("1.000", 1_000_000),
];

/// A fixed-bucket duration histogram. Buckets are stored as plain counts and summed into the
/// cumulative form Prometheus expects when the page is rendered.
pub struct Histogram {
    buckets: [AtomicU64; LATENCY_BUCKETS.len() + 1],
    count: AtomicU64,
    sum_micros: AtomicU64,
}

impl Default for Histogram {
    fn default() -> Histogram {
        Histogram { buckets: std::array::from_fn(|_| AtomicU64::new(0)), count: AtomicU64::new(0), sum_micros: AtomicU64::new(0) }
    }
}

impl Histogram {
    pub fn observe(&self, elapsed: Duration) {
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let index = LATENCY_BUCKETS.iter().position(|(_, bound)| micros <= *bound).unwrap_or(LATENCY_BUCKETS.len());
        if let Some(bucket) = self.buckets.get(index) {
            bucket.fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        // `fetch_update` is `try_update` since Rust 1.99. The closure always returns `Some`, so
        // an `Err` (the sum left as it was) cannot happen; the check keeps that explicit.
        let updated = self.sum_micros.try_update(Ordering::Relaxed, Ordering::Relaxed, |sum| Some(sum.saturating_add(micros)));
        debug_assert!(updated.is_ok(), "the sum_micros closure always returns Some");
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    fn render(&self, out: &mut String, name: &str, help: &str) {
        out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} histogram\n"));
        let mut cumulative = 0;
        for (index, (label, _)) in LATENCY_BUCKETS.iter().enumerate() {
            cumulative += self.buckets.get(index).map(|bucket| bucket.load(Ordering::Relaxed)).unwrap_or(0);
            out.push_str(&format!("{name}_bucket{{le=\"{label}\"}} {cumulative}\n"));
        }
        let count = self.count();
        out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {count}\n"));
        out.push_str(&format!("{name}_sum {}\n", seconds(self.sum_micros.load(Ordering::Relaxed))));
        out.push_str(&format!("{name}_count {count}\n"));
    }
}

/// Microseconds as a Prometheus seconds value, without floating point rounding.
fn seconds(micros: u64) -> String {
    format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000)
}

/// Gauges read from the state under its lock when the page is rendered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gauges {
    pub sessions: u64,
    pub participants: u64,
    pub leases: u64,
    /// Applied operations held in memory across sessions for catch-up.
    pub journal_ops: u64,
    /// Size of the journal files on the volume; zero without a store.
    pub journal_bytes: u64,
    /// Size of the base workbooks on the volume; zero without a store.
    pub base_bytes: u64,
}

/// Counters of one process.
#[derive(Default)]
pub struct Metrics {
    pub connections: AtomicU64,
    pub ops: AtomicU64,
    pub applied: AtomicU64,
    pub commits: AtomicU64,
    pub dropped: AtomicU64,
    pub auth_failures: AtomicU64,
    /// Connections that came back with a non-zero `lastVersion`.
    pub resumes: AtomicU64,
    /// Catch-ups served from the journal tail.
    pub catchups: AtomicU64,
    /// Catch-ups that needed the whole workbook.
    pub resync_snapshots: AtomicU64,
    pub snapshots: AtomicU64,
    /// Commits the cabinet accepted (`INTEGRATION.md` §5.2).
    pub cabinet_published: AtomicU64,
    /// Commits the cabinet refused with `409` (the file changed elsewhere).
    pub cabinet_conflicts: AtomicU64,
    /// Commits the cabinet did not store: a wrong token, a size refusal, an exhausted retry or any
    /// other refusal.
    pub cabinet_errors: AtomicU64,
    /// Unix seconds of the last written snapshot; `0` when none has been written.
    pub last_snapshot_at: AtomicU64,
    /// `op` frame → `applied` written back to its sender.
    pub op_latency: Histogram,
    /// Rejections by wire code (`forbidden`, `stale`, …).
    rejections: Mutex<BTreeMap<String, u64>>,
}

impl Metrics {
    pub fn rejection(&self, code: &str) {
        if let Ok(mut codes) = self.rejections.lock() {
            *codes.entry(code.to_string()).or_insert(0) += 1;
        }
    }

    pub fn render(&self, gauges: &Gauges, version: &str) -> String {
        let mut out = String::with_capacity(2048);
        out.push_str("# HELP gridcraft_collab_info Build information.\n");
        out.push_str("# TYPE gridcraft_collab_info gauge\n");
        out.push_str(&format!("gridcraft_collab_info{{version=\"{version}\"}} 1\n"));
        gauge(&mut out, "gridcraft_collab_sessions_open", "Open documents.", gauges.sessions);
        gauge(&mut out, "gridcraft_collab_participants", "Participants of all open documents.", gauges.participants);
        gauge(&mut out, "gridcraft_collab_leases", "Live file leases.", gauges.leases);
        gauge(&mut out, "gridcraft_collab_connections", "Open WebSocket connections.", self.connections.load(Ordering::Relaxed));
        gauge(&mut out, "gridcraft_collab_journal_ops", "Operations held in the in-memory journal.", gauges.journal_ops);
        gauge(&mut out, "gridcraft_collab_journal_bytes", "Size of the journal files on the volume.", gauges.journal_bytes);
        gauge(&mut out, "gridcraft_collab_base_bytes", "Size of the base workbooks on the volume.", gauges.base_bytes);
        gauge(
            &mut out,
            "gridcraft_collab_last_snapshot_timestamp_seconds",
            "Unix time of the last written snapshot.",
            self.last_snapshot_at.load(Ordering::Relaxed),
        );
        counter(&mut out, "gridcraft_collab_ops_total", "Operations received.");
        out.push_str(&format!("gridcraft_collab_ops_total {}\n", self.ops.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_applied_total", "Operations applied and broadcast.");
        out.push_str(&format!("gridcraft_collab_applied_total {}\n", self.applied.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_commits_total", "Commits served.");
        out.push_str(&format!("gridcraft_collab_commits_total {}\n", self.commits.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_snapshots_total", "Snapshots written.");
        out.push_str(&format!("gridcraft_collab_snapshots_total {}\n", self.snapshots.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_cabinet_published_total", "Commits the cabinet accepted.");
        out.push_str(&format!("gridcraft_collab_cabinet_published_total {}\n", self.cabinet_published.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_cabinet_conflicts_total", "Commits refused by the cabinet as an external edit.");
        out.push_str(&format!("gridcraft_collab_cabinet_conflicts_total {}\n", self.cabinet_conflicts.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_cabinet_errors_total", "Commits the cabinet did not store.");
        out.push_str(&format!("gridcraft_collab_cabinet_errors_total {}\n", self.cabinet_errors.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_resumes_total", "Connections that returned with a known version (reconnects).");
        out.push_str(&format!("gridcraft_collab_resumes_total {}\n", self.resumes.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_catchups_total", "Reconnects caught up from the journal tail.");
        out.push_str(&format!("gridcraft_collab_catchups_total {}\n", self.catchups.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_resync_snapshots_total", "Catch-ups that needed the whole workbook.");
        out.push_str(&format!("gridcraft_collab_resync_snapshots_total {}\n", self.resync_snapshots.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_auth_failures_total", "Connections refused before the handshake.");
        out.push_str(&format!("gridcraft_collab_auth_failures_total {}\n", self.auth_failures.load(Ordering::Relaxed)));
        counter(&mut out, "gridcraft_collab_dropped_frames_total", "Frames dropped for a slow connection.");
        out.push_str(&format!("gridcraft_collab_dropped_frames_total {}\n", self.dropped.load(Ordering::Relaxed)));
        self.op_latency.render(&mut out, "gridcraft_collab_op_latency_seconds", "Server time from an op frame to its applied reply.");
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
        let page = metrics.render(&Gauges { sessions: 1, participants: 2, leases: 1, ..Gauges::default() }, "0.16.0");
        assert!(page.contains("gridcraft_collab_sessions_open 1"));
        assert!(page.contains("gridcraft_collab_rejections_total{code=\"stale\"} 2"));
        assert!(page.contains("gridcraft_collab_rejections_total{code=\"forbidden\"} 1"));
    }

    #[test]
    fn latency_buckets_are_cumulative() {
        let histogram = Histogram::default();
        histogram.observe(Duration::from_micros(500));
        histogram.observe(Duration::from_millis(3));
        histogram.observe(Duration::from_millis(700));
        let mut out = String::new();
        histogram.render(&mut out, "latency_seconds", "help");
        assert!(out.contains("latency_seconds_bucket{le=\"0.001\"} 1"), "{out}");
        assert!(out.contains("latency_seconds_bucket{le=\"0.005\"} 2"), "{out}");
        assert!(out.contains("latency_seconds_bucket{le=\"0.500\"} 2"), "{out}");
        assert!(out.contains("latency_seconds_bucket{le=\"1.000\"} 3"), "{out}");
        assert!(out.contains("latency_seconds_bucket{le=\"+Inf\"} 3"), "{out}");
        assert!(out.contains("latency_seconds_count 3"), "{out}");
        assert!(out.contains("latency_seconds_sum 0.703500"), "{out}");
    }

    #[test]
    fn journal_gauges_reach_the_page() {
        let metrics = Metrics::default();
        let page = metrics.render(&Gauges { journal_ops: 12, journal_bytes: 4096, ..Gauges::default() }, "0.16.0");
        assert!(page.contains("gridcraft_collab_journal_ops 12"), "{page}");
        assert!(page.contains("gridcraft_collab_journal_bytes 4096"), "{page}");
        assert!(page.contains("gridcraft_collab_op_latency_seconds_bucket{le=\"+Inf\"} 0"), "{page}");
    }
}
