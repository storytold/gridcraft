#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

#[test]
fn help_exits_before_opening_files_or_starting_the_desktop() {
    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_gridcraft"))
            .args([flag, "/path/that/does/not/exist.xlsx"])
            .env("GRIDCRAFT_NO_PREFS", "1")
            .output()
            .expect("launch desktop help");
        assert!(output.status.success(), "{flag}: {:?}", output.status);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("usage: gridcraft"), "{stdout}");
        assert!(stdout.contains("--help"), "{stdout}");
        assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    }
}
