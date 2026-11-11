use std::{
    ffi::c_int,
    process::{Child, Command, Stdio},
};

pub const ARG: &str = "--mesa-watchdog";

pub fn tie_to_app(child: &Child) {
    use std::os::unix::process::CommandExt;

    let Ok(exe) = std::env::current_exe() else { return };
    let spawned = Command::new(exe)
        .arg(ARG)
        .arg(std::process::id().to_string())
        .arg(child.id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    match spawned {
        Ok(mut watchdog) => {
            let waiting = std::thread::Builder::new()
                .name("watchdog-reaper".into())
                .spawn(move || {
                    let _ = watchdog.wait();
                });
            if let Err(e) = waiting {
                eprintln!("Mesa: can't wait for the program that ends the embedding server: {e}");
            }
        }
        Err(e) => eprintln!("Mesa: can't start the program that ends the embedding server if Mesa is killed: {e}"),
    }
}

pub fn run_if_asked() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let [_, flag, app, server] = args.as_slice() else {
        return false;
    };
    if flag != ARG {
        return false;
    }
    if let (Ok(app), Ok(server)) = (app.parse::<c_int>(), server.parse::<c_int>()) {
        watch(app, server);
    }
    true
}

#[cfg(target_os = "macos")]
fn watch(app: c_int, server: c_int) {
    unsafe {
        let queue = libc::kqueue();
        if queue < 0 {
            return;
        }
        let event = |pid: c_int| libc::kevent {
            ident: pid as usize,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT | libc::EV_RECEIPT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let changes = [event(app), event(server)];
        let mut results = [event(0), event(0)];
        let registered = libc::kevent(queue, changes.as_ptr(), 2, results.as_mut_ptr(), 2, std::ptr::null());
        for result in results.iter().take(registered.max(0) as usize) {
            let gone = result.flags & libc::EV_ERROR != 0 && result.data != 0;
            if gone && result.ident == app as usize {
                libc::kill(server, libc::SIGKILL);
                libc::close(queue);
                return;
            }
            if gone && result.ident == server as usize {
                libc::close(queue);
                return;
            }
        }
        let mut ended = [event(0)];
        loop {
            let got = libc::kevent(queue, std::ptr::null(), 0, ended.as_mut_ptr(), 1, std::ptr::null());
            if got < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if got > 0 && ended[0].ident == app as usize {
                libc::kill(server, libc::SIGKILL);
            }
            break;
        }
        libc::close(queue);
    }
}

#[cfg(target_os = "linux")]
fn watch(app: c_int, server: c_int) {
    use std::{thread, time::Duration};

    let errno = || std::io::Error::last_os_error().raw_os_error();
    unsafe {
        let open = |pid: c_int| libc::syscall(libc::SYS_pidfd_open, pid, 0) as c_int;
        let server_fd = open(server);
        if server_fd < 0 {
            if errno() == Some(libc::ENOSYS) {
                return poll_by_signal(app, server);
            }
            return;
        }
        let app_fd = open(app);
        if app_fd < 0 {
            if errno() == Some(libc::ESRCH) {
                kill(server_fd);
            }
            libc::close(server_fd);
            return;
        }
        let mut fds = [
            libc::pollfd {
                fd: app_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: server_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            let got = libc::poll(fds.as_mut_ptr(), 2, -1);
            if got < 0 && errno() == Some(libc::EINTR) {
                continue;
            }
            if got > 0 && fds[0].revents != 0 && fds[1].revents == 0 {
                kill(server_fd);
            }
            break;
        }
        libc::close(app_fd);
        libc::close(server_fd);
    }

    unsafe fn kill(fd: c_int) {
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
    }

    fn poll_by_signal(app: c_int, server: c_int) {
        let alive = |pid: c_int| unsafe {
            libc::kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        };
        while alive(server) {
            if !alive(app) {
                unsafe { libc::kill(server, libc::SIGKILL) };
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}
