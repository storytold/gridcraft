//! The client against the fake worker: no Python needed, on every system.
#![allow(clippy::disallowed_methods)] // the clock: the client and its tests exist only off wasm

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gridcraft_core::{CellError, Value};

use super::*;
use crate::fake::{FakeHandle, FakeTransport, FakeWorker};
use crate::protocol::{Input, MAX_LINE_BYTES, Output, PyError, Reply, Request};

fn limits(timeout_ms: u64) -> Limits {
    Limits { timeout: Duration::from_millis(timeout_ms), start_timeout: Duration::ZERO, ..Limits::default() }
}

fn client(t: FakeTransport, timeout_ms: u64) -> (Client, FakeHandle) {
    let h = t.handle();
    (Client::new(Box::new(t), limits(timeout_ms)), h)
}

/// Answers `code` as a number when it parses as one, echoes the input `A1` for `xl`, and raises
/// a NameError on line 2 for anything else.
fn calculator() -> FakeTransport {
    FakeTransport::answering(|code, inputs| match code {
        "xl" => Ok(Output::Value(inputs.get("A1").map(|i| i.value.clone()).unwrap_or_default())),
        c => c.parse::<f64>().map(|n| Output::Value(Value::Number(n))).map_err(|_| PyError {
            message: "NameError: name 'y' is not defined".into(),
            line: Some(2),
            ..PyError::default()
        }),
    })
}

fn no_inputs() -> BTreeMap<String, Input> {
    BTreeMap::new()
}

fn ok_value(o: &Outcome) -> Value {
    match o {
        Outcome::Ok { output: Output::Value(v), .. } => v.clone(),
        other => panic!("not a value: {other:?}"),
    }
}

#[test]
fn values_round_trip_on_one_worker() {
    let (mut c, h) = client(calculator(), 5_000);
    assert_eq!(ok_value(&c.run("41.5", no_inputs()).unwrap()), Value::Number(41.5));
    let mut inputs = BTreeMap::new();
    let range = gridcraft_core::Array::new(1, 2, vec![Value::text("a"), Value::Error(CellError::Div0)]).map(Value::from).unwrap();
    inputs.insert("A1".to_string(), Input::new(range.clone()));
    assert_eq!(ok_value(&c.run("xl", inputs).unwrap()), range);
    assert_eq!(
        c.run("y", no_inputs()).unwrap(),
        Outcome::PythonError {
            error: PyError { message: "NameError: name 'y' is not defined".into(), line: Some(2), ..PyError::default() },
            stdout: String::new()
        }
    );
    // A Python error keeps the worker (and its globals).
    assert!(c.alive());
    assert_eq!(ok_value(&c.reset().unwrap()), Value::Empty);
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
    assert_eq!(h.starts(), 1, "every request ran on the same worker");
    let kinds: Vec<bool> = h.requests().iter().map(|r| matches!(r, Request::Reset { .. })).collect();
    assert_eq!(kinds, [false, false, false, true, false]);
    assert_eq!(c.self_test(), Some(&crate::protocol::SelfTest::NotChecked));
}

#[test]
fn a_reset_with_no_worker_needs_none() {
    let (mut c, h) = client(calculator(), 5_000);
    assert_eq!(ok_value(&c.reset().unwrap()), Value::Empty);
    assert_eq!(h.starts(), 0);
}

#[test]
fn one_request_at_a_time() {
    let (mut c, _h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            w.stall();
        }),
        5_000,
    );
    let id = c.submit_run("1", no_inputs()).unwrap();
    assert_eq!(c.submit_run("2", no_inputs()), Err(Busy));
    assert_eq!(c.submit_reset(), Err(Busy));
    assert_eq!(c.stop(), Some(id));
    assert!(!c.busy() && !c.alive());
}

#[test]
fn a_stalling_worker_times_out_and_cannot_move_the_deadline() {
    // The worker reads the request, then sends nothing; polling in small steps doesn't extend
    // the deadline either.
    let (mut c, h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            let _ = w.next_request();
            w.stall();
        }),
        300,
    );
    let t = Instant::now();
    let id = c.submit_run("while True: pass", no_inputs()).unwrap();
    let result = loop {
        if let Some(r) = c.poll(Duration::from_millis(20)) {
            break r;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "no timeout");
    };
    assert_eq!(result, RunResult { id, outcome: Outcome::TimedOut });
    assert!(t.elapsed() >= Duration::from_millis(300), "{:?}", t.elapsed());
    assert!(!c.alive(), "killed at the deadline");
    // The next request gets a fresh worker (which stalls as well).
    let _ = c.submit_run("1", no_inputs()).unwrap();
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_worker_line_never_extends_the_deadline() {
    // The old protocol had `extend` and a second `ready` that reset the clock: both now kill.
    for forged in [r#"{"type":"extend","id":1,"ms":600000}"#, r#"{"type":"ready","selfTest":{"status":"notChecked"}}"#] {
        let (mut c, h) = client(
            FakeTransport::new(move |mut w| {
                w.ready();
                let _ = w.next_request();
                w.write_raw(format!("{forged}\n").as_bytes());
                w.stall();
            }),
            2_000,
        );
        let t = Instant::now();
        let outcome = c.run("1", no_inputs()).unwrap();
        assert!(matches!(outcome, Outcome::ProtocolViolation(_)), "{forged}: {outcome:?}");
        assert!(t.elapsed() < Duration::from_secs(2), "killed at once, not at the deadline");
        assert!(!c.alive());
        assert_eq!(h.starts(), 1);
    }
}

#[test]
fn a_100_mb_line_is_cut_at_the_cap() {
    let (sent_tx, sent_rx) = mpsc::channel();
    let t = FakeTransport::new(move |mut w| {
        if w.start_number() > 1 {
            // The fresh worker behaves.
            w.ready();
            while let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Number(1.0))));
            }
            return;
        }
        w.ready();
        let _ = w.next_request();
        let _ = sent_tx.send(w.flood(100 * 1024 * 1024, b'x'));
    });
    let (mut c, h) = client(t, 20_000);
    let started = Instant::now();
    assert_eq!(c.run("1", no_inputs()).unwrap(), Outcome::OutputTooLarge);
    assert!(started.elapsed() < Duration::from_secs(15));
    let sent = sent_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(sent < 100 * 1024 * 1024, "the reader stopped reading: the flood broke off after {sent} bytes");
    assert!(sent >= MAX_LINE_BYTES, "{sent}");
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_smaller_line_cap_applies_to_every_line() {
    let t = FakeTransport::new(|mut w| {
        // `ready` itself is over a 16-byte cap.
        w.ready();
        w.stall();
    });
    let h = t.handle();
    let mut c = Client::new(Box::new(t), Limits { line_bytes: 16, ..limits(2_000) });
    assert_eq!(c.run("1", no_inputs()).unwrap(), Outcome::OutputTooLarge);
    assert_eq!(h.starts(), 1);
}

/// A worker that misbehaves on its first start only, then answers every run with 7.
fn first_start(misbehave: impl Fn(&mut FakeWorker, u64) + Send + Sync + 'static) -> FakeTransport {
    FakeTransport::new(move |mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            if w.start_number() == 1 {
                misbehave(&mut w, req.id());
                w.stall();
                return;
            }
            w.done(req.id(), Ok(Output::Value(Value::Number(7.0))));
        }
    })
}

/// What a fake worker does with a request's id.
type Misbehaviour = Box<dyn Fn(&mut FakeWorker, u64) + Send + Sync>;

#[test]
fn forged_wrong_id_and_malformed_replies_kill_the_worker() {
    let cases: Vec<(&str, Misbehaviour)> = vec![
        (
            "wrong id",
            Box::new(|w: &mut FakeWorker, id| {
                let _ = w.done(id + 1, Ok(Output::Value(Value::Number(1.0))));
            }),
        ),
        (
            "old id",
            Box::new(|w: &mut FakeWorker, id| {
                let _ = w.done(id.wrapping_sub(1), Ok(Output::Value(Value::Number(1.0))));
            }),
        ),
        (
            "malformed JSON",
            Box::new(|w: &mut FakeWorker, _| {
                let _ = w.write_raw(b"{\"type\":\"done\",\n");
            }),
        ),
        (
            "not a reply",
            Box::new(|w: &mut FakeWorker, _| {
                let _ = w.write_raw(b"hello from print()\n");
            }),
        ),
        (
            "bad shape",
            Box::new(|w: &mut FakeWorker, id| {
                let _ = w.write_raw(
                    format!("{{\"type\":\"done\",\"id\":{id},\"ok\":true,\"value\":{{\"array\":{{\"rows\":1000,\"cols\":1000,\"data\":[1]}}}}}}\n")
                        .as_bytes(),
                );
            }),
        ),
        (
            "forged ready",
            Box::new(|w: &mut FakeWorker, _| {
                let _ = w.ready();
            }),
        ),
    ];
    for (what, misbehave) in cases {
        let (mut c, h) = client(first_start(misbehave), 5_000);
        let outcome = c.run("1", no_inputs()).unwrap();
        assert!(matches!(outcome, Outcome::ProtocolViolation(_)), "{what}: {outcome:?}");
        assert!(!c.alive(), "{what}");
        assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(7.0), "{what}");
        assert_eq!(h.starts(), 2, "{what}: the next request ran on a fresh worker");
    }
}

#[test]
fn an_answer_before_ready_kills_the_worker() {
    let (mut c, _h) = client(
        FakeTransport::new(|mut w| {
            let req = w.next_request();
            w.done(req.map(|r| r.id()).unwrap_or(1), Ok(Output::Value(Value::Number(1.0))));
            w.stall();
        }),
        5_000,
    );
    assert!(matches!(c.run("1", no_inputs()).unwrap(), Outcome::ProtocolViolation(_)));
}

#[test]
fn unsolicited_lines_and_second_replies_kill_an_idle_worker() {
    // A second reply right after the first, and a line out of nowhere a little later.
    for second in [true, false] {
        let t = FakeTransport::new(move |mut w| {
            w.ready();
            while let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Number(w.start_number() as f64))));
                if w.start_number() == 1 {
                    if second {
                        w.done(req.id(), Ok(Output::Value(Value::Number(99.0))));
                    } else {
                        std::thread::sleep(Duration::from_millis(50));
                        w.write_raw(b"{\"type\":\"ready\",\"selfTest\":{\"status\":\"passed\"}}\n");
                    }
                    w.stall();
                    return;
                }
            }
        });
        let (mut c, h) = client(t, 5_000);
        assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
        // Idle: the line arrives and the worker goes.
        let t = Instant::now();
        while c.alive() && t.elapsed() < Duration::from_secs(5) {
            assert_eq!(c.poll(Duration::from_millis(100)), None);
        }
        assert!(!c.alive(), "second reply {second}: killed");
        assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(2.0), "second reply {second}: a fresh worker");
        assert_eq!(h.starts(), 2);
    }
}

#[test]
fn an_unsolicited_line_is_found_when_the_next_request_goes_out() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            w.done(req.id(), Ok(Output::Value(Value::Number(w.start_number() as f64))));
            if w.start_number() == 1 {
                w.done(req.id(), Ok(Output::Value(Value::Number(99.0))));
            }
        }
    });
    let (mut c, h) = client(t, 5_000);
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
    // Give the extra line time to reach the client, without polling.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(2.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_full_pipe_never_blocks_the_caller() {
    // The worker never reads: a request far bigger than a pipe's buffer can't be written.
    let (mut c, _h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            w.stall();
        }),
        400,
    );
    let code = "x".repeat(4 * 1024 * 1024);
    let t = Instant::now();
    let id = c.submit_run(&code, no_inputs()).unwrap();
    assert!(t.elapsed() < Duration::from_millis(250), "submit waited {:?}", t.elapsed());
    let t = Instant::now();
    assert_eq!(c.poll(Duration::ZERO), None);
    assert!(t.elapsed() < Duration::from_millis(50), "poll waited {:?}", t.elapsed());
    assert_eq!(c.wait(), Some(RunResult { id, outcome: Outcome::TimedOut }));
    // Dropping the client is immediate too, with the writer still stuck.
    let t = Instant::now();
    drop(c);
    assert!(t.elapsed() < Duration::from_millis(250));
}

#[test]
fn a_worker_that_exits_early() {
    // Before it is ready: it failed to start.
    let (mut c, _h) = client(FakeTransport::new(|_w| {}), 5_000);
    let outcome = c.run("1", no_inputs()).unwrap();
    assert!(matches!(&outcome, Outcome::FailedToStart(m) if m.contains("fake worker")), "{outcome:?}");
    assert_eq!(outcome.cell_error(), Some(CellError::Connect));
    // After it is ready, mid-run: it crashed.
    let (mut c, h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            if w.start_number() == 1 {
                let _ = w.next_request();
                return;
            }
            while let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Bool(true))));
            }
        }),
        5_000,
    );
    let outcome = c.run("1", no_inputs()).unwrap();
    assert!(matches!(outcome, Outcome::Crashed(_)), "{outcome:?}");
    assert_eq!(outcome.cell_error(), Some(CellError::Python));
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Bool(true));
    assert_eq!(h.starts(), 2);
}

/// Drawing a figure again goes only to the worker holding it: one that stopped while idle took the
/// figure with it, and starting a fresh one would start Python for nothing.
#[test]
fn a_worker_that_exits_while_idle_is_not_started_again_to_draw_a_figure() {
    let (mut c, h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            if let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Number(1.0))));
            }
            // Exits after one request.
        }),
        5_000,
    );
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
    // However soon the client sees it go, a request sent before then is answered by the worker's going
    // and the next finds none. At no point does anything start.
    let until = Instant::now() + Duration::from_secs(5);
    while c.submit_render([0, 0, 0], 10, 10).is_some() {
        assert!(Instant::now() < until, "the worker's end was never seen");
        while c.poll(Duration::from_millis(50)).is_none() {
            assert!(Instant::now() < until, "the request was never answered");
        }
    }
    assert_eq!(h.starts(), 1, "nothing started");
    assert!(!c.alive());
}

#[test]
fn a_running_worker_draws_a_figure_again() {
    let (mut c, h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            while let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Empty)));
            }
        }),
        5_000,
    );
    assert_eq!(c.submit_render([0, 0, 0], 10, 10), None, "no worker, so no figure");
    assert_eq!(h.starts(), 0);
    ok_value(&c.run("1", no_inputs()).unwrap());
    let id = c.submit_render([0, 0, 0], 10, 10).unwrap();
    assert!(c.poll(Duration::from_secs(5)).is_some_and(|r| r.id == id));
    assert_eq!(h.starts(), 1);
}

#[test]
fn a_worker_that_exits_while_idle_is_replaced() {
    let (mut c, h) = client(
        FakeTransport::new(|mut w| {
            w.ready();
            if let Some(req) = w.next_request() {
                w.done(req.id(), Ok(Output::Value(Value::Number(w.start_number() as f64))));
            }
            // Exits after one request.
        }),
        5_000,
    );
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(1.0));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(ok_value(&c.run("1", no_inputs()).unwrap()), Value::Number(2.0));
    assert_eq!(h.starts(), 2);
}

#[test]
fn a_worker_over_its_memory_limit_is_killed() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        let _ = w.next_request();
        w.stall();
    });
    let h = t.handle();
    let mut c = Client::new(Box::new(t), Limits { memory_bytes: 1_000, ..limits(5_000) });
    h.set_rss(2_000);
    let t = Instant::now();
    assert_eq!(c.run("1", no_inputs()).unwrap(), Outcome::OverMemory { rss_bytes: 2_000 });
    assert!(t.elapsed() < Duration::from_secs(4));
    assert!(!c.alive());
}

#[test]
fn a_transport_that_cannot_start() {
    struct Broken;
    impl Transport for Broken {
        fn start(&mut self) -> Result<crate::run::Pipes, String> {
            Err("no such interpreter".into())
        }
        fn kill(&mut self) {}
    }
    let mut c = Client::new(Box::new(Broken), Limits::default());
    let id = c.submit_run("1", no_inputs()).unwrap();
    assert_eq!(c.poll(Duration::ZERO), Some(RunResult { id, outcome: Outcome::FailedToStart("no such interpreter".into()) }));
    assert!(!c.busy());
}

#[test]
fn dropping_the_client_kills_the_worker() {
    let (gone_tx, gone_rx) = mpsc::channel();
    let t = FakeTransport::new(move |mut w| {
        w.ready();
        let _ = w.next_request();
        w.stall();
        let _ = gone_tx.send(w.killed());
    });
    let (mut c, _h) = client(t, 60_000);
    c.submit_run("1", no_inputs()).unwrap();
    assert_eq!(c.poll(Duration::from_millis(50)), None);
    drop(c);
    assert_eq!(gone_rx.recv_timeout(Duration::from_secs(5)), Ok(true));
}

#[test]
fn stdout_and_objects_come_through() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let reply =
                Reply::Done { id: req.id(), result: Ok(Output::object("dict", "{1: 2}")), stdout: "printed\n".into(), peak: None, touched: None };
            w.send(&reply);
        }
    });
    let (mut c, _h) = client(t, 5_000);
    assert_eq!(c.run("{1: 2}", no_inputs()).unwrap(), Outcome::Ok { output: Output::object("dict", "{1: 2}"), stdout: "printed\n".into() });
}

#[test]
fn limits_are_clamped() {
    let l = Limits { timeout: Duration::MAX, start_timeout: Duration::MAX, memory_bytes: 0, line_bytes: usize::MAX }.clamped();
    assert_eq!(l.timeout, crate::run::MAX_TIMEOUT);
    assert_eq!(l.line_bytes, MAX_LINE_BYTES);
    assert_eq!(Limits { timeout: Duration::ZERO, line_bytes: 0, ..Limits::default() }.clamped().timeout, Duration::from_millis(1));
}

/// A worker whose answers report a peak of the number its code holds.
fn reporting_peak() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let Request::Run { id, code, .. } = req else { continue };
            let peak = code.parse().ok();
            w.send(&Reply::Done { id, result: Ok(Output::Value(Value::Number(1.0))), stdout: String::new(), peak, touched: None });
        }
    })
}

#[test]
fn a_reply_over_the_memory_limit_counts_against_that_request() {
    let t = reporting_peak();
    let h = t.handle();
    let mut c = Client::new(Box::new(t), Limits { memory_bytes: 1_000, ..limits(5_000) });
    let fine = Outcome::Ok { output: Output::Value(Value::Number(1.0)), stdout: String::new() };
    assert_eq!(c.run("no peak reported", no_inputs()).unwrap(), fine);
    assert_eq!(c.run("1000", no_inputs()).unwrap(), fine, "at the limit is within it");
    assert_eq!(h.starts(), 1);
    assert_eq!(c.run("1001", no_inputs()).unwrap(), Outcome::OverMemory { rss_bytes: 1_001 });
    assert!(!c.alive(), "the worker that went over is gone");
    // The next request is clean, in a fresh worker.
    assert_eq!(c.run("5", no_inputs()).unwrap(), fine);
    assert_eq!(h.starts(), 2);
}
