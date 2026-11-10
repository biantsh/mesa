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

pub fn below_the_page() {
    imp::below_the_page();
}

static KEEPING_PACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn keep_pace(on: bool) {
    if KEEPING_PACE.swap(on, std::sync::atomic::Ordering::SeqCst) != on {
        imp::keep_pace(on);
    }
}

#[cfg(windows)]
pub fn never_throttle(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    imp::throttling(windows::Win32::Foundation::HANDLE(child.as_raw_handle()), true);
}

#[cfg(windows)]
mod imp {
    use windows::Win32::{
        Foundation::HANDLE,
        System::Threading::{
            GetCurrentProcess, GetCurrentThread, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling,
            SetProcessInformation, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END,
            THREAD_PRIORITY_BELOW_NORMAL,
        },
    };

    pub fn below_the_page() {
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    }

    pub fn keep_pace(on: bool) {
        throttling(unsafe { GetCurrentProcess() }, on);
    }

    pub fn throttling(process: HANDLE, on: bool) {
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: if on {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
            StateMask: 0,
        };
        let _ = unsafe {
            SetProcessInformation(
                process,
                ProcessPowerThrottling,
                &state as *const _ as *const core::ffi::c_void,
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
    }

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
    use std::sync::Mutex;

    use objc2::{
        rc::Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    };
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    struct Activity(Retained<ProtocolObject<dyn NSObjectProtocol>>);

    unsafe impl Send for Activity {}

    pub struct Hold(Mutex<Option<Activity>>);

    impl Hold {
        pub const fn new() -> Self {
            Self(Mutex::new(None))
        }

        pub fn set(&self, on: bool) {
            let mut held = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let info = NSProcessInfo::processInfo();
            match (on, held.take()) {
                (true, None) => {
                    let reason = NSString::from_str("Indexing");
                    let options = NSActivityOptions::UserInitiatedAllowingIdleSystemSleep;
                    *held = Some(Activity(info.beginActivityWithOptions_reason(options, &reason)));
                }
                (false, Some(activity)) => unsafe { info.endActivity(&activity.0) },
                (_, unchanged) => *held = unchanged,
            }
        }
    }

    static HELD: Hold = Hold::new();

    pub fn keep_pace(on: bool) {
        HELD.set(on);
    }

    pub fn below_the_page() {
        let _ = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) };
    }

    pub fn give_way(on: bool) {
        let _ = unsafe { libc::setpriority(libc::PRIO_DARWIN_THREAD, 0, if on { libc::PRIO_DARWIN_BG } else { 0 }) };
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn give_way(_: bool) {}

    pub fn below_the_page() {}

    pub fn keep_pace(_: bool) {}
}
