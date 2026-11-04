use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("gridcraft-logging-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read log")
}

fn record(logger: &AppLogger, level: log::Level, target: &str, msg: &str) {
    use log::Log;
    logger.log(&log::Record::builder().level(level).target(target).args(format_args!("{msg}")).build());
}

#[test]
fn the_default_filter_shows_gridcraft_info_and_other_crates_warnings() {
    let f = Filter::parse(DEFAULT_FILTER);
    assert_eq!(f.level_for("gridcraft"), LevelFilter::Info);
    assert_eq!(f.level_for("gridcraft::control_server"), LevelFilter::Info);
    assert_eq!(f.level_for("gridcraft_engine::sample"), LevelFilter::Info);
    assert_eq!(f.level_for("gridcraft_ui_egui::control"), LevelFilter::Info);
    assert_eq!(f.level_for("wgpu_core::device"), LevelFilter::Warn);
    assert_eq!(f.level_for("naga"), LevelFilter::Warn);
    assert_eq!(f.max(), LevelFilter::Info);
}

#[test]
fn directives_follow_env_logger_and_the_most_specific_one_wins() {
    let f = Filter::parse("info,wgpu_core=error,gridcraft_calc=trace,gridcraft_calc::recalc=off");
    assert_eq!(f.level_for("eframe"), LevelFilter::Info);
    assert_eq!(f.level_for("wgpu_core::instance"), LevelFilter::Error);
    assert_eq!(f.level_for("gridcraft_calc::graph"), LevelFilter::Trace);
    assert_eq!(f.level_for("gridcraft_calc::recalc"), LevelFilter::Off);
    assert_eq!(f.max(), LevelFilter::Trace);
    // A module name is matched at `::` boundaries, not as a bare prefix.
    assert_eq!(f.level_for("wgpu_core_extra"), LevelFilter::Info);
    // A trailing `*` is a prefix.
    assert_eq!(Filter::parse("gridcraft*=debug").level_for("gridcraft_xlsx::read"), LevelFilter::Debug);
    assert_eq!(Filter::parse("gridcraft*=debug").level_for("eframe"), LevelFilter::Error);
    // A bare target name sets that target to the most verbose level, as env_logger does.
    assert_eq!(Filter::parse("naga").level_for("naga::front"), LevelFilter::Trace);
    assert_eq!(Filter::parse("naga").level_for("eframe"), LevelFilter::Error);
    // Levels are case-insensitive and surrounding blanks are ignored.
    assert_eq!(Filter::parse(" WARN , gridcraft = Debug ").level_for("gridcraft"), LevelFilter::Debug);
    assert_eq!(Filter::parse(" WARN , gridcraft = Debug ").level_for("eframe"), LevelFilter::Warn);
}

#[test]
fn hostile_specs_never_panic_and_fall_back_sensibly() {
    let long = "x".repeat(100_000);
    let many = "a=info,".repeat(10_000);
    for spec in [
        "",
        ",,,",
        "=",
        "==",
        "=debug",
        "gridcraft=",
        "nonsense=loud",
        "🦀=info",
        "*",
        "*=",
        "=*",
        "a=b=c",
        "warn,,gridcraft=DEBUG",
        "\0",
        "é::ü=trace",
        &long,
        &many,
    ] {
        let f = Filter::parse(spec);
        for target in ["", "gridcraft", "🦀", "a::b", "*"] {
            let _ = f.level_for(target);
        }
        let _ = f.max();
    }
    assert_eq!(Filter::parse("warn,,gridcraft=DEBUG").level_for("gridcraft"), LevelFilter::Debug);
    // An unknown level is skipped: that target keeps the default.
    assert_eq!(Filter::parse("nonsense=loud").level_for("nonsense"), LevelFilter::Error);
    assert_eq!(Filter::parse("gridcraft=").level_for("gridcraft"), LevelFilter::Error);
    assert_eq!(Filter::parse("=debug").max(), LevelFilter::Error);
    assert_eq!(Filter::parse("🦀=info").level_for("🦀::claw"), LevelFilter::Info);
}

#[test]
fn timestamps_are_utc_with_milliseconds() {
    assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_millis(1_791_446_357_728)), "2026-10-08T07:59:17.728Z");
    // Leap day, and the day after it.
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_709_164_800)), "2024-02-29T00:00:00.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_709_251_199)), "2024-02-29T23:59:59.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_709_251_200)), "2024-03-01T00:00:00.000Z");
    // 2000 is a leap year (divisible by 400), 2100 is not.
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(4_107_542_400)), "2100-03-01T00:00:00.000Z");
    // Before the epoch (a clock set wrong) clamps instead of panicking.
    assert_eq!(timestamp(UNIX_EPOCH - Duration::from_secs(5)), "1970-01-01T00:00:00.000Z");
    // A clock set absurdly far ahead (past chrono's own range) keeps its real year rather than
    // collapsing to the epoch. Windows' SystemTime can't hold such an instant, so it is built
    // with a checked add and only asserted where it exists.
    if let Some(far) = UNIX_EPOCH.checked_add(Duration::from_secs(u64::MAX / 2)) {
        assert_eq!(timestamp(far), "292277026596-12-04T15:30:07.000Z");
    }
    if let Some(far) = UNIX_EPOCH.checked_add(Duration::from_secs(10_000_000_000_000)) {
        assert_eq!(timestamp(far), "318857-05-20T17:46:40.000Z");
    }
    // Years past 9999 keep the plain zero-padded year (no `+`).
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(253_402_300_800)), "10000-01-01T00:00:00.000Z");
}

#[test]
fn a_line_carries_time_level_thread_target_and_message() {
    assert_eq!(
        format_line("2026-10-08T07:59:17.728Z", log::Level::Warn, "main", "gridcraft", "book.xlsx: file not found"),
        "2026-10-08T07:59:17.728Z WARN  [main] gridcraft: book.xlsx: file not found\n"
    );
    // A multi-line message keeps its lines; the record still ends in one newline.
    assert!(format_line("t", log::Level::Error, "w", "x", "a\nb\n").ends_with("x: a\nb\n"));
}

#[test]
fn rotation_keeps_the_previous_logs_and_starts_an_empty_file() {
    let dir = temp_dir("rotate");
    for run in 1..=4 {
        let (path, _file) = rotate(&dir).expect("rotate");
        assert_eq!(path, dir.join(LOG_FILE));
        assert_eq!(read(&path), "");
        std::fs::write(&path, format!("run {run}")).expect("write");
    }
    assert_eq!(read(&dir.join(LOG_FILE)), "run 4");
    assert_eq!(read(&dir.join("gridcraft.1.log")), "run 3");
    assert_eq!(read(&dir.join("gridcraft.2.log")), "run 2");
    assert!(!dir.join("gridcraft.3.log").exists(), "only {KEEP} old logs are kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotation_into_an_unusable_directory_is_an_error_not_a_panic() {
    let dir = temp_dir("blocked");
    std::fs::create_dir_all(dir.parent().expect("parent")).expect("tmp");
    std::fs::write(&dir, "a file where the directory should be").expect("block");
    assert!(rotate(&dir).is_err());
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn lines_logged_before_the_file_exists_are_written_when_it_is_attached() {
    let dir = temp_dir("pending");
    let (path, file) = rotate(&dir).expect("rotate");
    let mut sink = Sink::new(MAX_FILE_BYTES);
    sink.write("early 1\n");
    sink.write("early 2\n");
    sink.attach(file);
    sink.write("late\n");
    assert_eq!(read(&path), "early 1\nearly 2\nlate\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_pending_buffer_is_bounded_and_says_how_much_it_dropped() {
    let dir = temp_dir("bounded");
    let (path, file) = rotate(&dir).expect("rotate");
    let mut sink = Sink::new(MAX_FILE_BYTES);
    for i in 0..MAX_PENDING + 5 {
        sink.write(&format!("line {i}\n"));
    }
    sink.attach(file);
    let text = read(&path);
    assert_eq!(text.lines().filter(|l| l.starts_with("line ")).count(), MAX_PENDING);
    assert!(text.contains("5 earlier log lines were dropped"), "{text}");
    assert!(text.contains("line 0\n") && !text.contains(&format!("line {MAX_PENDING}\n")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exactly_a_full_pending_buffer_drops_nothing() {
    let dir = temp_dir("full");
    let (path, file) = rotate(&dir).expect("rotate");
    let mut sink = Sink::new(MAX_FILE_BYTES);
    for i in 0..MAX_PENDING {
        sink.write(&format!("line {i}\n"));
    }
    sink.attach(file);
    let text = read(&path);
    assert_eq!(text.lines().count(), MAX_PENDING);
    assert!(!text.contains("dropped"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_file_nothing_is_kept_in_memory() {
    let mut sink = Sink::new(MAX_FILE_BYTES);
    sink.write("early\n");
    sink.no_file();
    sink.write("late\n");
    assert!(sink.pending.is_empty() && sink.dropped == 0);
}

#[test]
fn the_file_stops_at_its_size_cap_with_one_note() {
    let dir = temp_dir("cap");
    let (path, file) = rotate(&dir).expect("rotate");
    let mut sink = Sink::new(100);
    sink.attach(file);
    for i in 0..50 {
        sink.write(&format!("line {i:02} with some padding\n"));
    }
    let text = read(&path);
    assert_eq!(text.lines().filter(|l| l.starts_with("line ")).count(), 3, "{text}");
    assert_eq!(text.matches("log file reached").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_logger_filters_records_and_writes_them_to_the_attached_file() {
    use log::Log;
    let dir = temp_dir("logger");
    let logger = AppLogger::new(Filter::parse(DEFAULT_FILTER), false);
    record(&logger, log::Level::Info, "gridcraft", "before the settings directory");
    record(&logger, log::Level::Info, "wgpu_core::device", "too chatty");
    let path = logger.attach_dir(&dir).expect("attach");
    assert_eq!(path, dir.join(LOG_FILE));
    record(&logger, log::Level::Warn, "wgpu_hal::vulkan", "a real warning");
    record(&logger, log::Level::Error, "gridcraft", "internal error: boom");
    record(&logger, log::Level::Debug, "gridcraft", "below info");
    let text = read(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].contains(" INFO  [") && lines[0].ends_with("gridcraft: before the settings directory"), "{text}");
    assert!(lines[1].ends_with("wgpu_hal::vulkan: a real warning"), "{text}");
    assert!(lines[2].contains(" ERROR [") && lines[2].ends_with("gridcraft: internal error: boom"), "{text}");
    assert!(logger.enabled(&log::Metadata::builder().level(log::Level::Info).target("gridcraft_ui_egui").build()));
    assert!(!logger.enabled(&log::Metadata::builder().level(log::Level::Info).target("naga").build()));
    // Attaching again rotates: the first log becomes `.1`.
    logger.attach_dir(&dir).expect("attach again");
    assert_eq!(read(&dir.join("gridcraft.1.log")), text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn records_from_many_threads_are_whole_lines() {
    let dir = temp_dir("threads");
    let logger = std::sync::Arc::new(AppLogger::new(Filter::parse("info"), false));
    let path = logger.attach_dir(&dir).expect("attach");
    let workers: Vec<_> = (0..8)
        .map(|t| {
            let l = std::sync::Arc::clone(&logger);
            std::thread::spawn(move || {
                for i in 0..100 {
                    record(&l, log::Level::Info, "gridcraft", &format!("thread {t} record {i}"));
                }
            })
        })
        .collect();
    for w in workers {
        w.join().expect("worker");
    }
    let text = read(&path);
    assert_eq!(text.lines().count(), 800);
    assert!(text.lines().all(|l| l.contains(" gridcraft: thread ")), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn attaching_to_an_unusable_directory_reports_an_error_and_keeps_logging() {
    let dir = temp_dir("unusable");
    std::fs::create_dir_all(dir.parent().expect("parent")).expect("tmp");
    std::fs::write(&dir, "not a directory").expect("block");
    let logger = AppLogger::new(Filter::parse("info"), false);
    assert!(logger.attach_dir(&dir).is_err());
    record(&logger, log::Level::Error, "x", "still fine");
    // Nothing waits in memory for a file that won't come.
    assert!(logger.sink.lock().expect("sink").pending.is_empty());
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn a_poisoned_sink_still_logs() {
    let dir = temp_dir("poison");
    let logger = std::sync::Arc::new(AppLogger::new(Filter::parse("info"), false));
    let l = std::sync::Arc::clone(&logger);
    let _ = std::thread::spawn(move || {
        let _guard = l.sink.lock().expect("lock");
        panic!("poison the sink");
    })
    .join();
    assert!(logger.sink.is_poisoned());
    let path = logger.attach_dir(&dir).expect("attach");
    record(&logger, log::Level::Warn, "gridcraft", "after the poison");
    assert!(read(&path).contains("gridcraft: after the poison"));
    let _ = std::fs::remove_dir_all(&dir);
}
