//! `io::read_file` only reads ordinary files and never trusts the size the file system reports:
//! a FIFO (or any special file) reports a tiny size while supplying unbounded data.

use gridcraft_engine::io::read_file;

#[test]
fn read_file_returns_ordinary_file_unchanged() {
    let dir = std::env::temp_dir().join(format!("gridcraft-read-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plain.bin");
    let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &data).unwrap();
    let got = read_file(path.to_str().unwrap()).unwrap();
    assert_eq!(got, data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn read_file_rejects_fifo() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("gridcraft-read-fifo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pipe");
    let _ = std::fs::remove_file(&path);
    let status = std::process::Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(status.success());
    let writer_path = path.clone();
    // The writer blocks until a reader opens the FIFO and ignores a closed pipe; it is never
    // joined, so a reader that rejects early can't hang the test.
    std::thread::spawn(move || {
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&writer_path) {
            let chunk = vec![7u8; 64 * 1024];
            for _ in 0..128 {
                if f.write_all(&chunk).is_err() {
                    break;
                }
            }
        }
    });
    let res = read_file(path.to_str().unwrap());
    assert!(res.is_err(), "non-regular file accepted: read {} bytes", res.map(|b| b.len()).unwrap_or(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn read_file_rejects_fifo_without_writer_promptly() {
    let dir = std::env::temp_dir().join(format!("gridcraft-read-fifo-idle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pipe");
    let _ = std::fs::remove_file(&path);
    let status = std::process::Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(status.success());
    // No writer ever opens the FIFO: a blocking open would hang, so read on a thread and time out.
    let (tx, rx) = std::sync::mpsc::channel();
    let reader_path = path.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read_file(reader_path.to_str().unwrap()).is_err());
    });
    let res = rx.recv_timeout(std::time::Duration::from_secs(5));
    if res.is_err() {
        // Unblock the stuck reader so the process can exit.
        let _ = std::fs::OpenOptions::new().write(true).open(&path);
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(res, Ok(true), "read_file on an idle FIFO must fail promptly instead of blocking");
}
