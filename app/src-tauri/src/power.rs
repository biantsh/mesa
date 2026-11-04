pub fn give_way() {
    imp::give_way(true);
}

pub fn stop_giving_way() {
    imp::give_way(false);
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

#[cfg(not(windows))]
mod imp {
    pub fn give_way(_: bool) {}
}
