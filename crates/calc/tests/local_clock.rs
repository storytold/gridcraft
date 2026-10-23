//! Each timezone runs in a child process; the test runner's environment is never changed.
#[cfg(unix)]
#[test]
fn clock_uses_the_local_timezone() {
    use gridcraft_core::date::{local_serial_from_unix_ms, unix_now_ms};
    if std::env::var_os("GRIDCRAFT_CLOCK_TEST_CHILD").is_some() {
        let actual = gridcraft_calc::now_serial();
        let expected = local_serial_from_unix_ms(unix_now_ms().unwrap()).unwrap();
        assert!((actual - expected).abs() * 86400.0 < 2.0, "actual {actual}, local {expected}");
        let zone = std::env::var("TZ").unwrap();
        for (instant, offset) in [
            (
                1_704_067_200_000.0,
                match zone.as_str() {
                    "Europe/Rome" => 60.0,
                    "America/New_York" => -300.0,
                    _ => 330.0,
                },
            ),
            (
                1_719_792_000_000.0,
                match zone.as_str() {
                    "Europe/Rome" => 120.0,
                    "America/New_York" => -240.0,
                    _ => 330.0,
                },
            ),
        ] {
            let expected = gridcraft_core::date::serial_from_unix_ms(instant) + offset / 1440.0;
            assert!((local_serial_from_unix_ms(instant).unwrap() - expected).abs() < 1e-9);
        }
        return;
    }
    for zone in ["Europe/Rome", "America/New_York", "Asia/Kolkata"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "clock_uses_the_local_timezone", "--nocapture"])
            .env("GRIDCRAFT_CLOCK_TEST_CHILD", "1")
            .env("TZ", zone)
            .status()
            .unwrap();
        assert!(status.success(), "timezone {zone}");
    }
}
