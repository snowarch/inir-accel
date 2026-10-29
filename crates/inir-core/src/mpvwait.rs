//! `mpv-wait <socket> <reject-file> [seconds]`: MpvIpc's wait for the iNiR mpv wrapper, out of its inline Python.
//! The wrapper refuses a show ani-cli resolved wrong by writing the reject file; otherwise mpv opens its IPC socket.
//! Exit 0 once the socket accepts a connection, 2 with the reject file's text when the wrapper refused, 1 at the deadline.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub fn run_cli(args: &[String]) -> i32 {
    let (socket, reject) = match args {
        [s, r] | [s, r, _] => (s, r),
        _ => {
            eprintln!("usage: inir-accel mpv-wait <socket> <reject-file> [seconds]");
            return 2;
        }
    };
    let secs = args.get(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(180);
    let _ = std::fs::remove_file(reject);
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if std::path::Path::new(reject).exists() {
            if let Ok(text) = std::fs::read(reject) {
                let _ = std::io::stdout().write_all(&text);
                let _ = std::fs::remove_file(reject);
            }
            return 2;
        }
        if UnixStream::connect(socket).is_ok() {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    1
}
