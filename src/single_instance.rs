//! Keeps one DevSticky running per user on macOS.
//!
//! GApplication's uniqueness needs a D-Bus session bus, which macOS doesn't
//! have, so every launch (Finder, Spotlight, the login item, a terminal) would
//! start another copy with its own menu-bar icon. Instead, the first copy holds
//! a lock file and listens on a Unix socket; later launches send their
//! command-line arguments over the socket and exit.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub enum Instance {
    /// This is the only copy. Keep `lock` alive for the life of the process;
    /// arguments from later launches arrive on the receiver.
    Primary {
        _lock: File,
        launches: async_channel::Receiver<Vec<String>>,
    },
    /// Another copy is running and has been handed our arguments: exit.
    Secondary,
}

fn dir() -> PathBuf {
    gtk::glib::home_dir()
        .join("Library/Caches")
        .join(crate::APP_ID)
}

/// `None` if locking is not possible; then just run without the check.
pub fn claim(args: &[String]) -> Option<Instance> {
    let dir = dir();
    std::fs::create_dir_all(&dir).ok()?;
    let socket = dir.join("instance.sock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))
        .ok()?;

    match lock.try_lock() {
        Ok(()) => {
            // We hold the lock, so any socket file left over is stale.
            let _ = std::fs::remove_file(&socket);
            let (tx, rx) = async_channel::unbounded();
            match UnixListener::bind(&socket) {
                Ok(listener) => {
                    std::thread::spawn(move || serve(listener, tx));
                }
                Err(e) => eprintln!("devsticky: listening on {} failed: {e}", socket.display()),
            }
            Some(Instance::Primary { _lock: lock, launches: rx })
        }
        Err(TryLockError::WouldBlock) => {
            forward(&socket, args);
            Some(Instance::Secondary)
        }
        Err(TryLockError::Error(e)) => {
            eprintln!("devsticky: single-instance lock failed: {e}");
            None
        }
    }
}

/// Sends `args` (one per line) to the running copy. It may still be starting
/// up, so keep trying for a few seconds.
fn forward(socket: &std::path::Path, args: &[String]) {
    for _ in 0..50 {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let mut msg = args.join("\n");
            msg.push('\n');
            if let Err(e) = stream.write_all(msg.as_bytes()) {
                eprintln!("devsticky: talking to the running copy failed: {e}");
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("devsticky: DevSticky is already running but not responding");
}

fn serve(listener: UnixListener, tx: async_channel::Sender<Vec<String>>) {
    for stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let args: Vec<String> = BufReader::new(stream)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.is_empty())
            .collect();
        if tx.send_blocking(args).is_err() {
            return;
        }
    }
}
