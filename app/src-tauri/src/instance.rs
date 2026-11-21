//! Keeps one copy of Mesa running per data folder.
//!
//! Two lock files in the data folder decide which copy runs:
//! 
//!  - `running.lock` is held by the copy that runs Mesa for as long as it lives. Only that copy
//!   opens the index, so two copies can never write it at once.
//! 
//!  - `starting.lock` is held while a copy starts up. Every launch takes it before looking at
//!   `running.lock`, and the running copy lets go of it only once it can take the arguments of
//!   later launches. So a launch that finds Mesa running knows it can hand over straight away.

use std::{
    fs::{self, File, TryLockError},
    io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const RUNNING_FILE: &str = "running.lock";
const STARTING_FILE: &str = "starting.lock";
/// How long a launch waits for another copy to finish starting before giving up on it.
const WAIT_FOR_START: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(50);

pub enum Launch {
    /// This copy runs Mesa. Call [`Claim::ready`] once it can take later launches' arguments.
    First(Claim),
    /// Another copy runs Mesa and is ready for this launch's arguments, or is stuck starting up.
    Later,
}

/// The locks of the copy that runs Mesa, while it starts up.
#[derive(Default)]
pub struct Claim {
    running: Option<File>,
    starting: Option<File>,
}

/// Holds the running lock until dropped, which Tauri does in `cleanup_before_exit`, so a restart
/// after an update can start the new copy.
pub struct Running {
    _file: Option<File>,
    socket: Option<PathBuf>,
}

impl tauri::Resource for Running {}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = fs::remove_file(socket);
        }
    }
}

pub fn claim(dir: &Path) -> Launch {
    claim_within(dir, WAIT_FOR_START).unwrap_or_else(|e| {
        eprintln!("Mesa: couldn't check whether Mesa is already running, so starting anyway. {e}");
        Launch::First(Claim::default())
    })
}

fn claim_within(dir: &Path, wait: Duration) -> io::Result<Launch> {
    fs::create_dir_all(dir)?;
    let starting = lock_file(&dir.join(STARTING_FILE))?;
    let deadline = Instant::now() + wait;
    loop {
        match starting.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(POLL),
            Err(TryLockError::WouldBlock) => {
                eprintln!("Mesa: another copy of Mesa is still starting up, so this one is leaving it to that.");
                return Ok(Launch::Later);
            }
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
    let running = lock_file(&dir.join(RUNNING_FILE))?;
    match running.try_lock() {
        Ok(()) => Ok(Launch::First(Claim {
            running: Some(running),
            starting: Some(starting),
        })),
        Err(TryLockError::WouldBlock) => Ok(Launch::Later),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn lock_file(path: &Path) -> io::Result<File> {
    File::options().read(true).write(true).create(true).truncate(false).open(path)
}

impl Claim {
    /// Lets later launches through. On macOS, first opens the socket they hand their arguments to.
    pub fn ready(self, app: &tauri::AppHandle, dir: &Path) -> Running {
        #[cfg(target_os = "macos")]
        let socket = {
            let app = app.clone();
            match handoff::listen(dir, move |args| crate::startup::another_launch(&app, &args)) {
                Ok(socket) => Some(socket),
                Err(e) => {
                    eprintln!("Mesa: later launches can't reach this copy. {e}");
                    None
                }
            }
        };
        #[cfg(not(target_os = "macos"))]
        let socket = {
            let _ = (app, dir);
            None
        };
        self.into_running(socket)
    }

    fn into_running(self, socket: Option<PathBuf>) -> Running {
        drop(self.starting);
        Running {
            _file: self.running,
            socket,
        }
    }
}

#[cfg(target_os = "macos")]
pub use handoff::hand_over;

#[cfg(target_os = "macos")]
mod handoff {
    use std::{
        ffi::{CStr, OsStr},
        io::{self, Read, Write},
        os::unix::{
            ffi::OsStrExt,
            net::{UnixListener, UnixStream},
        },
        path::{Path, PathBuf},
        time::Duration,
    };

    /// A socket path can be at most 104 bytes, too short for some data folders, so the socket
    /// lives in the user's temporary folder, named after the data folder. That folder is only
    /// open to its user, and macOS gives the same one to every process of that user, whether
    /// launchd or the Finder started it.
    fn socket_path(dir: &Path) -> io::Result<PathBuf> {
        let mut buf = [0 as libc::c_char; 1024];
        let len = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr(), buf.len()) };
        if len == 0 || len > buf.len() {
            return Err(io::Error::other("macOS didn't say where the user's temporary folder is"));
        }
        let temp = unsafe { CStr::from_ptr(buf.as_ptr()) };
        let name = format!("mesa-{:016x}.sock", fnv1a(dir.as_os_str().as_bytes()));
        Ok(Path::new(OsStr::from_bytes(temp.to_bytes())).join(name))
    }

    /// Stable across Rust versions, unlike `DefaultHasher`, so a copy of Mesa built with another
    /// compiler finds the same socket.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    }

    /// Only the copy holding the running lock calls this, so any socket already there is left
    /// over from a crash and safe to replace.
    pub fn listen(dir: &Path, on_launch: impl Fn(Vec<String>) + Send + 'static) -> io::Result<PathBuf> {
        let path = socket_path(dir)?;
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::thread::Builder::new().name("later-launches".into()).spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut text = String::new();
                if stream.read_to_string(&mut text).is_ok() {
                    on_launch(text.split('\0').map(String::from).collect());
                }
            }
        })?;
        Ok(path)
    }

    pub fn hand_over(dir: &Path, args: &[String]) {
        let sent = socket_path(dir)
            .and_then(UnixStream::connect)
            .and_then(|mut stream| stream.write_all(args.join("\0").as_bytes()));
        if let Err(e) = sent {
            eprintln!("Mesa: couldn't reach the copy of Mesa that's already running. {e}");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn hands_arguments_to_the_running_copy() {
            let dir = super::super::tests::temp_dir("handoff");
            let (tx, rx) = std::sync::mpsc::channel();
            let socket = listen(&dir, move |args| tx.send(args).unwrap()).unwrap();
            let args = vec!["mesa-app".to_owned(), "--search".to_owned(), "with space".to_owned()];
            hand_over(&dir, &args);
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), args);
            assert!(socket.as_os_str().len() < 104);
            let _ = std::fs::remove_file(socket);
        }

        #[test]
        fn each_data_folder_has_its_own_socket() {
            assert_ne!(socket_path(Path::new("/a")).unwrap(), socket_path(Path::new("/b")).unwrap());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-instance-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn first(launch: Launch) -> Claim {
        match launch {
            Launch::First(claim) => claim,
            Launch::Later => panic!("expected this launch to run Mesa"),
        }
    }

    fn running(claim: Claim) -> Running {
        claim.into_running(None)
    }

    #[test]
    fn a_later_launch_waits_until_the_running_copy_is_ready() {
        let dir = temp_dir("waits");
        let claim = first(claim_within(&dir, Duration::from_secs(10)).unwrap());
        let later = {
            let dir = dir.clone();
            thread::spawn(move || claim_within(&dir, Duration::from_secs(10)).unwrap())
        };
        thread::sleep(Duration::from_millis(300));
        assert!(!later.is_finished(), "the later launch should wait while the first starts up");
        let _running = running(claim);
        assert!(matches!(later.join().unwrap(), Launch::Later));
    }

    #[test]
    fn a_later_launch_gives_up_on_a_copy_stuck_starting() {
        let dir = temp_dir("stuck");
        let _claim = first(claim_within(&dir, Duration::from_secs(10)).unwrap());
        assert!(matches!(claim_within(&dir, Duration::from_millis(200)).unwrap(), Launch::Later));
    }

    #[test]
    fn the_next_launch_runs_once_the_running_copy_ends() {
        let dir = temp_dir("ends");
        drop(running(first(claim_within(&dir, Duration::from_secs(10)).unwrap())));
        first(claim_within(&dir, Duration::from_secs(10)).unwrap());
    }

    #[test]
    fn copies_with_different_data_folders_both_run() {
        let _a = first(claim_within(&temp_dir("folder-a"), Duration::from_secs(10)).unwrap());
        first(claim_within(&temp_dir("folder-b"), Duration::from_secs(10)).unwrap());
    }
}
