pub fn give_way() {
    imp::give_way(true);
}

thread_local! {
    static FOLLOWING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn follow(on: bool) {
    FOLLOWING.with(|state| {
        if state.get() != on {
            imp::give_way(on);
            state.set(on);
        }
    });
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END,
    };

    pub fn give_way(on: bool) {
        let mode = if on {
            THREAD_MODE_BACKGROUND_BEGIN
        } else {
            THREAD_MODE_BACKGROUND_END
        };
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), mode) };
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn give_way(on: bool) {
        let _ = unsafe { libc::setpriority(libc::PRIO_DARWIN_THREAD, 0, if on { libc::PRIO_DARWIN_BG } else { 0 }) };
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn give_way(_: bool) {}
}
