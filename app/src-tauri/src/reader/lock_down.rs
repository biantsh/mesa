pub const MOST_MEMORY: u64 = if cfg!(test) { 1 << 30 } else { 4 << 30 };

pub fn lock_down() {
    if let Err(e) = imp::limit_memory(MOST_MEMORY) {
        eprintln!("Mesa: the file reader runs without a memory limit: {e}");
    }
    if let Err(e) = imp::no_writing() {
        eprintln!("Mesa: the file reader can still write files: {e}");
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            Security::{
                CreateWellKnownSid, PSID, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES, SetTokenInformation,
                TOKEN_ADJUST_DEFAULT, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel, WinLowLabelSid,
            },
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
                    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JobObjectExtendedLimitInformation, SetInformationJobObject,
                },
                Threading::{GetCurrentProcess, OpenProcessToken},
            },
        },
        core::PCWSTR,
    };

    const SE_GROUP_INTEGRITY: u32 = 0x20;

    pub fn no_writing() -> Result<(), String> {
        unsafe {
            let mut sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
            let mut size = sid.len() as u32;
            let low = PSID(sid.as_mut_ptr().cast());
            CreateWellKnownSid(WinLowLabelSid, None, Some(low), &mut size).map_err(|e| e.to_string())?;
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT | TOKEN_QUERY, &mut token)
                .map_err(|e| e.to_string())?;
            let label = TOKEN_MANDATORY_LABEL {
                Label: SID_AND_ATTRIBUTES {
                    Sid: low,
                    Attributes: SE_GROUP_INTEGRITY,
                },
            };
            let lowered = SetTokenInformation(
                token,
                TokenIntegrityLevel,
                &label as *const TOKEN_MANDATORY_LABEL as *const c_void,
                size_of::<TOKEN_MANDATORY_LABEL>() as u32 + size,
            );
            let _ = CloseHandle(token);
            lowered.map_err(|e| e.to_string())
        }
    }

    pub fn limit_memory(bytes: u64) -> Result<(), String> {
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).map_err(|e| e.to_string())?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            limits.ProcessMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| e.to_string())?;
            AssignProcessToJobObject(job, GetCurrentProcess()).map_err(|e| e.to_string())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::{
        ffi::{CStr, c_char, c_int},
        thread,
        time::Duration,
    };

    const SANDBOX_NAMED: u64 = 1;
    const NO_WRITE_EXCEPT_TEMPORARY: &CStr = c"no-write-except-temporary";

    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    pub fn no_writing() -> Result<(), String> {
        unsafe {
            let mut error: *mut c_char = std::ptr::null_mut();
            if sandbox_init(NO_WRITE_EXCEPT_TEMPORARY.as_ptr(), SANDBOX_NAMED, &mut error) == 0 {
                return Ok(());
            }
            let why = if error.is_null() {
                "no reason given".to_owned()
            } else {
                let why = CStr::from_ptr(error).to_string_lossy().into_owned();
                sandbox_free_error(error);
                why
            };
            Err(why)
        }
    }

    pub fn limit_memory(bytes: u64) -> Result<(), String> {
        thread::Builder::new()
            .name("memory-watch".into())
            .spawn(move || watch(bytes))
            .map(drop)
            .map_err(|e| e.to_string())
    }

    fn watch(bytes: u64) {
        while footprint() <= bytes {
            thread::sleep(Duration::from_millis(250));
        }
        eprintln!("Mesa: the file reader needed more than {} GB, so it stops", bytes >> 30);
        unsafe { libc::_exit(70) }
    }

    fn footprint() -> u64 {
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        let read = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V2,
                (&mut info as *mut libc::rusage_info_v2).cast(),
            )
        };
        if read == 0 { info.ri_phys_footprint } else { 0 }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{c_long, c_ulong};

    const CREATE_RULESET_VERSION: u32 = 1;
    const RULE_PATH_BENEATH: c_long = 1;

    const WRITE_FILE: u64 = 1 << 1;
    const REMOVE_DIR: u64 = 1 << 4;
    const REMOVE_FILE: u64 = 1 << 5;
    const MAKE_CHAR: u64 = 1 << 6;
    const MAKE_DIR: u64 = 1 << 7;
    const MAKE_REG: u64 = 1 << 8;
    const MAKE_SOCK: u64 = 1 << 9;
    const MAKE_FIFO: u64 = 1 << 10;
    const MAKE_BLOCK: u64 = 1 << 11;
    const MAKE_SYM: u64 = 1 << 12;
    const REFER: u64 = 1 << 13;
    const TRUNCATE: u64 = 1 << 14;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    pub fn no_writing() -> Result<(), String> {
        let failed = |what: &str| format!("{what}: {}", std::io::Error::last_os_error());
        unsafe {
            let abi = libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            );
            if abi < 1 {
                return Err(failed("this Linux can't restrict it"));
            }
            let mut handled = WRITE_FILE
                | REMOVE_DIR
                | REMOVE_FILE
                | MAKE_CHAR
                | MAKE_DIR
                | MAKE_REG
                | MAKE_SOCK
                | MAKE_FIFO
                | MAKE_BLOCK
                | MAKE_SYM;
            if abi >= 2 {
                handled |= REFER;
            }
            if abi >= 3 {
                handled |= TRUNCATE;
            }
            let attr = RulesetAttr {
                handled_access_fs: handled,
            };
            let ruleset = libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr as *const RulesetAttr,
                size_of::<RulesetAttr>(),
                0u32,
            ) as libc::c_int;
            if ruleset < 0 {
                return Err(failed("can't make the rules"));
            }
            let devices = libc::open(c"/dev".as_ptr(), libc::O_PATH | libc::O_CLOEXEC);
            if devices >= 0 {
                let rule = PathBeneathAttr {
                    allowed_access: WRITE_FILE,
                    parent_fd: devices,
                };
                libc::syscall(
                    libc::SYS_landlock_add_rule,
                    ruleset,
                    RULE_PATH_BENEATH,
                    &rule as *const PathBeneathAttr,
                    0u32,
                );
                libc::close(devices);
            }
            let restricted = libc::prctl(
                libc::PR_SET_NO_NEW_PRIVS,
                1 as c_ulong,
                0 as c_ulong,
                0 as c_ulong,
                0 as c_ulong,
            ) == 0
                && libc::syscall(libc::SYS_landlock_restrict_self, ruleset, 0u32) == 0;
            let why = (!restricted).then(|| failed("can't apply the rules"));
            libc::close(ruleset);
            why.map_or(Ok(()), Err)
        }
    }

    pub fn limit_memory(bytes: u64) -> Result<(), String> {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_DATA, &mut limit) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        limit.rlim_cur = limit.rlim_max.min(bytes as libc::rlim_t);
        if unsafe { libc::setrlimit(libc::RLIMIT_DATA, &limit) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
}
