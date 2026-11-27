use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
};

use super::{
    Role, lock_down,
    wire::{self, Call, FromHelper, Job, Reply, ToHelper},
};
use crate::{embed::Item, index::lock};

pub const ARG: &str = "--mesa-reader";

struct Running {
    answers: Sender<bool>,
    cancelled: Arc<AtomicBool>,
}

pub fn serve(role: Role) -> ! {
    quiet_crashes();
    lock_down::lock_down();
    let output = Arc::new(Mutex::new(take_output()));
    if send(&output, &FromHelper::Hello).is_err() {
        std::process::exit(0);
    }
    if role == Role::Previews {
        let _ = thread::Builder::new().name("warm-up".into()).spawn(|| {
            crate::power::below_the_page();
            crate::document_preview::warm_up();
        });
    }
    let running: Arc<Mutex<HashMap<u64, Running>>> = Arc::default();
    let mut input = BufReader::new(std::io::stdin().lock());
    loop {
        let Ok(Some(frame)) = wire::read_frame(&mut input) else {
            std::process::exit(0);
        };
        match ToHelper::decode(&frame) {
            Some(ToHelper::Start(id, job)) => start(id, job, &output, &running),
            Some(ToHelper::Answer(id, more)) => {
                if let Some(job) = lock(&running).get(&id) {
                    let _ = job.answers.send(more);
                }
            }
            Some(ToHelper::Cancel(id)) => {
                if let Some(job) = lock(&running).get(&id) {
                    job.cancelled.store(true, Ordering::SeqCst);
                    let _ = job.answers.send(false);
                }
            }
            None => std::process::exit(0),
        }
    }
}

fn start(id: u64, job: Job, output: &Arc<Mutex<File>>, running: &Arc<Mutex<HashMap<u64, Running>>>) {
    let (answers, answered) = channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    lock(running).insert(
        id,
        Running {
            answers,
            cancelled: Arc::clone(&cancelled),
        },
    );
    let (output, running) = (Arc::clone(output), Arc::clone(running));
    let started = thread::Builder::new().name(format!("read-{id}")).spawn(move || {
        crate::power::below_the_page();
        let ask = |call: Call| {
            if cancelled.load(Ordering::SeqCst) || send(&output, &FromHelper::Call(id, call)).is_err() {
                return false;
            }
            answered.recv().unwrap_or(false) && !cancelled.load(Ordering::SeqCst)
        };
        let reply = match catch_unwind(AssertUnwindSafe(|| execute(job, &ask))) {
            Ok(reply) => reply,
            Err(_) => std::process::abort(),
        };
        lock(&running).remove(&id);
        if send(&output, &FromHelper::Done(id, reply)).is_err() {
            std::process::exit(0);
        }
    });
    if started.is_err() {
        std::process::abort();
    }
}

fn send(output: &Mutex<File>, message: &FromHelper) -> std::io::Result<()> {
    let body = message.encode();
    let mut output = lock(output);
    wire::write_frame(&mut *output, &body)
}

pub fn execute(job: Job, call: &dyn Fn(Call) -> bool) -> Reply {
    let item = |path: String, words_only: bool| Item {
        path: PathBuf::from(path),
        words_only,
        shallow: false,
    };
    match job {
        Job::Prepare {
            path,
            code,
            words_only,
            read_text,
        } => {
            let mut words = String::new();
            let mut text = |locator: u64, rgb: Vec<u8>, width: u32, height: u32| {
                call(Call::Text {
                    locator,
                    rgb,
                    width,
                    height,
                });
            };
            let text = read_text.then_some(&mut text as &mut dyn FnMut(u64, Vec<u8>, u32, u32));
            let read = crate::embed::prepare(
                &item(path, words_only),
                code,
                text,
                &mut |unit| call(Call::Unit(unit)),
                &mut words,
            );
            Reply::Words(read.map(|()| words))
        }
        Job::Sample { path, code, from, to } => Reply::Slots(
            crate::embed::sample(&item(path, false), code, from as usize, to as usize, &mut |unit| {
                call(Call::Unit(unit))
            })
            .map(|slots| slots as u64),
        ),
        Job::Passages { path } => Reply::Done(crate::embed::passages(Path::new(&path), &mut |locator, text| {
            call(Call::Passage {
                locator,
                text: text.to_owned(),
            })
        })),
        Job::Render { path, size } => Reply::Bytes(guarded(|| crate::thumbnail::render(&path, size))),
        Job::RenderPage { path, size, page } => {
            Reply::Bytes(guarded(|| crate::document_preview::render_page(&path, size, page)))
        }
        Job::Paging { path } => Reply::Paging(
            guarded(|| crate::document_preview::paging(&path)).map(|paging| (paging.pages as u64, paging.note)),
        ),
        Job::PageTexts { path } => {
            Reply::Texts(guarded(|| crate::document_preview::page_texts(&path)).map(|texts| texts.to_vec()))
        }
        Job::Ping => Reply::Done(Ok(())),
        #[cfg(test)]
        Job::Test(trial) => trial.run(call),
    }
}

fn guarded<T>(make: impl FnOnce() -> Option<T>) -> Option<T> {
    catch_unwind(AssertUnwindSafe(make)).ok().flatten()
}

#[cfg(unix)]
fn take_output() -> File {
    use std::os::fd::FromRawFd;

    unsafe {
        let protocol = libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3);
        if protocol < 0 {
            std::process::exit(0);
        }
        libc::dup2(2, 1);
        File::from_raw_fd(protocol)
    }
}

#[cfg(windows)]
fn take_output() -> File {
    use std::os::windows::io::FromRawHandle;

    use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    unsafe {
        let Ok(protocol) = GetStdHandle(STD_OUTPUT_HANDLE) else {
            std::process::exit(0);
        };
        if let Ok(errors) = GetStdHandle(STD_ERROR_HANDLE) {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, errors);
        }
        File::from_raw_handle(protocol.0)
    }
}

#[cfg(windows)]
fn quiet_crashes() {
    use windows::Win32::System::{
        Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode},
        ErrorReporting::{WER_FAULT_REPORTING, WER_FAULT_REPORTING_NO_UI, WerSetFlags},
    };

    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        let _ = WerSetFlags(WER_FAULT_REPORTING(WER_FAULT_REPORTING_NO_UI));
    }
}

#[cfg(not(windows))]
fn quiet_crashes() {}
