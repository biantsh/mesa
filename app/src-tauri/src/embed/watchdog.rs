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
        Ok(watchdog) => drop(watchdog),
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
            if got < 0 && *libc::__error() == libc::EINTR {
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
