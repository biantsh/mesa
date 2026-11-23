mod helper;
mod lock_down;
mod wire;

use std::{
    collections::HashMap,
    fs,
    io::BufReader,
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use crossbeam_channel::{RecvTimeoutError, Sender, unbounded};

use crate::{
    document_preview::Paging,
    embed::{Item, ReadText, Unit},
    index::lock,
};
use wire::{Call, FromHelper, Job, Reply, ToHelper};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Indexing,
    Previews,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Indexing => "indexing",
            Role::Previews => "previews",
        }
    }

    fn named(name: &str) -> Self {
        if name == Role::Previews.name() {
            Role::Previews
        } else {
            Role::Indexing
        }
    }
}

const STARTING: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_millis(100);
const LOOK_EVERY: Duration = Duration::from_millis(500);
const MOST_REMEMBERED: usize = 1000;
const STOPPED: &str = "reading it was stopped";
const BROKE_BEFORE: &str = "reading it stopped Mesa's file reader before";

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    File(String),
    Reader(String),
}

impl Failure {
    pub fn into_text(self) -> String {
        match self {
            Failure::File(why) | Failure::Reader(why) => why,
        }
    }
}

pub struct Reader {
    role: Role,
    stall: Duration,
    #[cfg(test)]
    remote: bool,
    process: Mutex<Option<Arc<Process>>>,
    gate: Gate,
    next_id: AtomicU64,
    broke: Mutex<Vec<(String, Option<SystemTime>)>>,
}

static INDEXING: Reader = Reader::new(Role::Indexing, Duration::from_secs(180));
static PREVIEWS: Reader = Reader::new(Role::Previews, Duration::from_secs(60));

pub fn indexing() -> &'static Reader {
    &INDEXING
}

pub fn previews() -> &'static Reader {
    &PREVIEWS
}

pub fn start() {
    let started = thread::Builder::new().name("reader-start".into()).spawn(|| {
        for reader in [indexing(), previews()] {
            if let Err(e) = reader.current() {
                eprintln!("Mesa: {e}");
            }
        }
    });
    if let Err(e) = started {
        eprintln!("Mesa: can't start the file readers yet: {e}");
    }
}

pub fn run_if_asked() -> bool {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(helper::ARG) {
        return false;
    }
    helper::serve(Role::named(&args.next().unwrap_or_default()))
}

impl Reader {
    const fn new(role: Role, stall: Duration) -> Self {
        Self {
            role,
            stall,
            #[cfg(test)]
            remote: false,
            process: Mutex::new(None),
            gate: Gate::new(),
            next_id: AtomicU64::new(1),
            broke: Mutex::new(Vec::new()),
        }
    }

    pub fn prepare(
        &self,
        item: &Item,
        mut text: ReadText,
        emit: &mut dyn FnMut(Unit) -> bool,
        words: &mut String,
        stop: &dyn Fn() -> bool,
    ) -> Result<(), Failure> {
        let job = Job::Prepare {
            path: path_text(&item.path)?,
            code: crate::embed::code::is_code_path(&item.path),
            words_only: item.words_only,
            read_text: text.is_some(),
        };
        let mut calls = |call| match call {
            Call::Unit(unit) => emit(unit),
            Call::Text {
                locator,
                rgb,
                width,
                height,
            } => {
                if let Some(text) = text.as_deref_mut() {
                    text(locator, rgb, width, height);
                }
                true
            }
            Call::Passage { .. } => true,
        };
        match self.run(job, &mut calls, stop)? {
            Reply::Words(Ok(found)) => {
                words.push_str(&found);
                Ok(())
            }
            Reply::Words(Err(why)) => Err(Failure::File(why)),
            _ => Err(odd_answer()),
        }
    }

    pub fn sample(
        &self,
        item: &Item,
        from: usize,
        to: usize,
        emit: &mut dyn FnMut(Unit) -> bool,
    ) -> Result<usize, Failure> {
        let job = Job::Sample {
            path: path_text(&item.path)?,
            code: crate::embed::code::is_code_path(&item.path),
            from: from as u64,
            to: to as u64,
        };
        let mut calls = |call| match call {
            Call::Unit(unit) => emit(unit),
            _ => true,
        };
        match self.run(job, &mut calls, &|| false)? {
            Reply::Slots(Ok(slots)) => Ok(slots as usize),
            Reply::Slots(Err(why)) => Err(Failure::File(why)),
            _ => Err(odd_answer()),
        }
    }

    pub fn passages(&self, path: &Path, each: &mut dyn FnMut(u64, &str) -> bool) -> Result<(), String> {
        let job = Job::Passages {
            path: path_text(path).map_err(Failure::into_text)?,
        };
        let mut calls = |call| match call {
            Call::Passage { locator, text } => each(locator, &text),
            _ => true,
        };
        match self.run(job, &mut calls, &|| false).map_err(Failure::into_text)? {
            Reply::Done(done) => done,
            _ => Err(odd_answer().into_text()),
        }
    }

    pub fn render(&self, path: &str, size: u32) -> Option<Vec<u8>> {
        let path = path.to_owned();
        match self.ask(Job::Render { path, size })? {
            Reply::Bytes(bytes) => bytes,
            _ => None,
        }
    }

    pub fn render_page(&self, path: &str, size: u32, page: u32) -> Option<Vec<u8>> {
        let path = path.to_owned();
        match self.ask(Job::RenderPage { path, size, page })? {
            Reply::Bytes(bytes) => bytes,
            _ => None,
        }
    }

    pub fn paging(&self, path: &str) -> Option<Paging> {
        match self.ask(Job::Paging { path: path.to_owned() })? {
            Reply::Paging(paging) => paging.map(|(pages, note)| Paging {
                pages: pages as usize,
                note,
            }),
            _ => None,
        }
    }

    pub fn page_texts(&self, path: &str) -> Option<Vec<String>> {
        match self.ask(Job::PageTexts { path: path.to_owned() })? {
            Reply::Texts(texts) => texts,
            _ => None,
        }
    }

    fn ask(&self, job: Job) -> Option<Reply> {
        self.run(job, &mut |_| true, &|| false).ok()
    }

    fn run(&self, job: Job, calls: &mut dyn FnMut(Call) -> bool, stop: &dyn Fn() -> bool) -> Result<Reply, Failure> {
        #[cfg(test)]
        if !self.remote {
            let calls = std::cell::RefCell::new(calls);
            return Ok(helper::execute(job, &|call| (calls.borrow_mut())(call)));
        }
        if let Some(path) = job.path()
            && self.broke_before(path)
        {
            return Err(Failure::File(BROKE_BEFORE.into()));
        }
        let (mut handed, mut refused, mut alone) = (0, false, false);
        loop {
            let _turn = if alone { self.gate.alone() } else { self.gate.together() };
            let process = if alone { self.fresh() } else { self.current() }.map_err(Failure::Reader)?;
            let mut seen = 0;
            let mut pass_on = |call| {
                seen += 1;
                if seen <= handed {
                    return true;
                }
                handed += 1;
                let more = calls(call);
                refused |= !more;
                more
            };
            match process.attempt(self.id(), &job, &mut pass_on, stop) {
                Ok(reply) => return Ok(reply),
                Err(Attempt::Stopped) => return Err(Failure::File(STOPPED.into())),
                Err(Attempt::Died(_)) if refused => return Err(Failure::File(STOPPED.into())),
                Err(Attempt::Died(ending)) if alone => return Err(self.blame(&job, ending)),
                Err(Attempt::Died(_)) => alone = true,
            }
        }
    }

    fn broke_before(&self, path: &str) -> bool {
        let broke = lock(&self.broke);
        !broke.is_empty() && broke.contains(&(path.to_owned(), modified(path)))
    }

    fn blame(&self, job: &Job, ending: Ending) -> Failure {
        let answered = self
            .fresh()
            .ok()
            .and_then(|process| process.attempt(self.id(), &Job::Ping, &mut |_| false, &|| false).ok());
        if answered != Some(Reply::Done(Ok(()))) {
            return Failure::Reader("Mesa's file reader stops even with no file to read".into());
        }
        if let Some(path) = job.path() {
            let mut broke = lock(&self.broke);
            if broke.len() >= MOST_REMEMBERED {
                broke.clear();
            }
            broke.push((path.to_owned(), modified(path)));
        }
        Failure::File(match ending {
            Ending::Crashed => "reading it crashed Mesa's file reader".into(),
            Ending::Stuck => "reading it got stuck, so Mesa stopped it".into(),
        })
    }

    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn current(&self) -> Result<Arc<Process>, String> {
        let mut process = lock(&self.process);
        if let Some(running) = process.as_ref().filter(|process| process.alive()) {
            return Ok(Arc::clone(running));
        }
        let started = Process::spawn(self.role, self.stall)?;
        *process = Some(Arc::clone(&started));
        Ok(started)
    }

    fn fresh(&self) -> Result<Arc<Process>, String> {
        let mut process = lock(&self.process);
        if let Some(old) = process.take() {
            old.end(Ending::Crashed);
        }
        let started = Process::spawn(self.role, self.stall)?;
        *process = Some(Arc::clone(&started));
        Ok(started)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(process) = lock(&self.process).take() {
            process.end(Ending::Crashed);
        }
    }
}

fn path_text(path: &Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::File("its name isn't valid text".into()))
}

fn modified(path: &str) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

fn odd_answer() -> Failure {
    Failure::Reader("Mesa's file reader gave an answer that doesn't fit".into())
}

struct Turns {
    together: usize,
    waiting_alone: usize,
    alone: bool,
}

struct Gate {
    turns: Mutex<Turns>,
    changed: Condvar,
}

struct Turn<'a> {
    gate: &'a Gate,
    alone: bool,
}

impl Gate {
    const fn new() -> Self {
        Self {
            turns: Mutex::new(Turns {
                together: 0,
                waiting_alone: 0,
                alone: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn together(&self) -> Turn<'_> {
        let mut turns = lock(&self.turns);
        while turns.alone || turns.waiting_alone > 0 {
            turns = self.changed.wait(turns).unwrap_or_else(PoisonError::into_inner);
        }
        turns.together += 1;
        Turn {
            gate: self,
            alone: false,
        }
    }

    fn alone(&self) -> Turn<'_> {
        let mut turns = lock(&self.turns);
        turns.waiting_alone += 1;
        while turns.alone || turns.together > 0 {
            turns = self.changed.wait(turns).unwrap_or_else(PoisonError::into_inner);
        }
        turns.waiting_alone -= 1;
        turns.alone = true;
        Turn {
            gate: self,
            alone: true,
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut turns = lock(&self.gate.turns);
        if self.alone {
            turns.alone = false;
        } else {
            turns.together -= 1;
        }
        self.gate.changed.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Crashed,
    Stuck,
}

enum Attempt {
    Stopped,
    Died(Ending),
}

enum Heard {
    Call(Call),
    Done(Reply),
}

struct Slot {
    heard: Sender<Heard>,
    waiting: Option<Instant>,
}

struct Process {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    slots: Mutex<HashMap<u64, Slot>>,
    ending: Mutex<Option<Ending>>,
}

impl Process {
    fn spawn(role: Role, stall: Duration) -> Result<Arc<Self>, String> {
        let mut command = command(role)?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Mesa couldn't start its file reader: {e}"))?;
        #[cfg(windows)]
        crate::power::never_throttle(&child);
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Mesa couldn't talk to its file reader".into());
        };
        let process = Arc::new(Process {
            child: Mutex::new(child),
            input: Mutex::new(input),
            slots: Mutex::default(),
            ending: Mutex::new(None),
        });
        let (greeted, greeting) = mpsc::channel();
        let listening = Arc::clone(&process);
        let listener = thread::Builder::new()
            .name(format!("{}-reader", role.name()))
            .spawn(move || listening.listen(output, greeted));
        if listener.is_err() || greeting.recv_timeout(STARTING) != Ok(true) {
            process.end(Ending::Crashed);
            return Err("Mesa's file reader didn't start".into());
        }
        let watched = Arc::clone(&process);
        let watcher = thread::Builder::new()
            .name(format!("{}-reader-watch", role.name()))
            .spawn(move || watched.watch(stall));
        if watcher.is_err() {
            process.end(Ending::Crashed);
            return Err("Mesa couldn't watch its file reader".into());
        }
        Ok(process)
    }

    fn listen(&self, output: ChildStdout, greeted: mpsc::Sender<bool>) {
        let mut output = BufReader::with_capacity(1 << 16, output);
        let hello = matches!(
            wire::read_first_frame(&mut output),
            Ok(Some(frame)) if matches!(FromHelper::decode(&frame), Some(FromHelper::Hello))
        );
        let _ = greeted.send(hello);
        while hello && let Ok(Some(frame)) = wire::read_frame(&mut output) {
            let (id, heard) = match FromHelper::decode(&frame) {
                Some(FromHelper::Call(id, call)) => (id, Heard::Call(call)),
                Some(FromHelper::Done(id, reply)) => (id, Heard::Done(reply)),
                _ => break,
            };
            let done = matches!(heard, Heard::Done(_));
            let mut slots = lock(&self.slots);
            if let Some(slot) = slots.get_mut(&id)
                && slot.heard.send(heard).is_ok()
            {
                slot.waiting = None;
            }
            if done {
                slots.remove(&id);
            }
        }
        self.end(Ending::Crashed);
        let _ = lock(&self.child).wait();
    }

    fn watch(&self, stall: Duration) {
        while self.alive() {
            thread::sleep(LOOK_EVERY);
            let stuck = lock(&self.slots)
                .values()
                .any(|slot| slot.waiting.is_some_and(|since| since.elapsed() > stall));
            if stuck {
                self.end(Ending::Stuck);
            }
        }
    }

    fn alive(&self) -> bool {
        lock(&self.ending).is_none()
    }

    fn ending(&self) -> Ending {
        lock(&self.ending).unwrap_or(Ending::Crashed)
    }

    fn end(&self, ending: Ending) {
        lock(&self.ending).get_or_insert(ending);
        let _ = lock(&self.child).kill();
        lock(&self.slots).clear();
    }

    fn send(&self, message: &ToHelper) -> bool {
        let sent = wire::write_frame(&mut *lock(&self.input), &message.encode()).is_ok();
        if !sent {
            self.end(Ending::Crashed);
        }
        sent
    }

    fn attempt(
        &self,
        id: u64,
        job: &Job,
        calls: &mut dyn FnMut(Call) -> bool,
        stop: &dyn Fn() -> bool,
    ) -> Result<Reply, Attempt> {
        let (heard_in, heard) = unbounded();
        {
            let mut slots = lock(&self.slots);
            if !self.alive() {
                return Err(Attempt::Died(self.ending()));
            }
            slots.insert(
                id,
                Slot {
                    heard: heard_in,
                    waiting: Some(Instant::now()),
                },
            );
        }
        if !self.send(&ToHelper::Start(id, job.clone())) {
            return Err(Attempt::Died(self.ending()));
        }
        loop {
            match heard.recv_timeout(TICK) {
                Ok(Heard::Call(call)) => {
                    let more = calls(call);
                    if let Some(slot) = lock(&self.slots).get_mut(&id) {
                        slot.waiting = Some(Instant::now());
                    }
                    if !self.send(&ToHelper::Answer(id, more)) {
                        return Err(Attempt::Died(self.ending()));
                    }
                }
                Ok(Heard::Done(reply)) => return Ok(reply),
                Err(RecvTimeoutError::Timeout) => {
                    if stop() {
                        self.send(&ToHelper::Cancel(id));
                        return Err(Attempt::Stopped);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Err(Attempt::Died(self.ending())),
            }
        }
    }
}

#[cfg(not(test))]
fn command(role: Role) -> Result<Command, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Mesa can't find itself to start its file reader: {e}"))?;
    let mut command = Command::new(exe);
    command.arg(helper::ARG).arg(role.name());
    Ok(command)
}

#[cfg(test)]
fn command(role: Role) -> Result<Command, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .args([
            "reader::tests::serve_when_asked",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(tests::SERVE, role.name());
    Ok(command)
}

#[cfg(test)]
pub mod tests {
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use super::*;

    pub const SERVE: &str = "MESA_READER_TEST";

    #[test]
    fn serve_when_asked() {
        if let Some(role) = std::env::var_os(SERVE) {
            helper::serve(Role::named(&role.to_string_lossy()));
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Trial {
        Crash(String),
        Hang(String),
        Wait(String, u64),
        Read(String),
        Write(String),
        Remove(String),
        CallThenCrash(String, u64),
        Allocate(String),
    }

    impl Trial {
        pub fn path(&self) -> Option<&str> {
            match self {
                Trial::Crash(path)
                | Trial::Hang(path)
                | Trial::Wait(path, _)
                | Trial::Read(path)
                | Trial::Write(path)
                | Trial::Remove(path)
                | Trial::CallThenCrash(path, _)
                | Trial::Allocate(path) => Some(path),
            }
        }

        pub fn to_line(&self) -> String {
            match self {
                Trial::Crash(path) => format!("crash\t{path}"),
                Trial::Hang(path) => format!("hang\t{path}"),
                Trial::Wait(path, ms) => format!("wait\t{path}\t{ms}"),
                Trial::Read(path) => format!("read\t{path}"),
                Trial::Write(path) => format!("write\t{path}"),
                Trial::Remove(path) => format!("remove\t{path}"),
                Trial::CallThenCrash(path, calls) => format!("calls\t{path}\t{calls}"),
                Trial::Allocate(path) => format!("allocate\t{path}"),
            }
        }

        pub fn from_line(line: &str) -> Option<Self> {
            let mut parts = line.split('\t');
            let (kind, path) = (parts.next()?, parts.next()?.to_owned());
            let mut number = || parts.next()?.parse().ok();
            Some(match kind {
                "crash" => Trial::Crash(path),
                "hang" => Trial::Hang(path),
                "wait" => Trial::Wait(path, number()?),
                "read" => Trial::Read(path),
                "write" => Trial::Write(path),
                "remove" => Trial::Remove(path),
                "calls" => Trial::CallThenCrash(path, number()?),
                "allocate" => Trial::Allocate(path),
                _ => return None,
            })
        }

        pub fn run(&self, call: &dyn Fn(Call) -> bool) -> Reply {
            let crash = || unsafe { std::ptr::without_provenance_mut::<u8>(8).write_volatile(1) };
            Reply::Done(match self {
                Trial::Crash(_) => {
                    crash();
                    Ok(())
                }
                Trial::Hang(_) => loop {
                    thread::sleep(Duration::from_secs(3600));
                },
                Trial::Wait(_, ms) => {
                    thread::sleep(Duration::from_millis(*ms));
                    Ok(())
                }
                Trial::Read(path) => fs::read(path).map(drop).map_err(|e| e.to_string()),
                Trial::Write(path) => fs::write(path, "changed").map_err(|e| format!("{:?}", e.kind())),
                Trial::Remove(path) => fs::remove_file(path).map_err(|e| format!("{:?}", e.kind())),
                Trial::CallThenCrash(_, calls) => {
                    for locator in 0..*calls {
                        call(Call::Passage {
                            locator,
                            text: format!("passage {locator}"),
                        });
                    }
                    crash();
                    Ok(())
                }
                Trial::Allocate(_) => {
                    let big = vec![1u8; (lock_down::MOST_MEMORY + (1 << 30)) as usize];
                    std::hint::black_box(&big);
                    Ok(())
                }
            })
        }
    }

    fn spawning(role: Role, stall: Duration) -> Reader {
        let mut reader = Reader::new(role, stall);
        reader.remote = true;
        reader
    }

    fn folder(tag: &str) -> std::path::PathBuf {
        let beside_tests = std::env::current_exe().unwrap().parent().unwrap().to_owned();
        let dir = beside_tests.join(format!("mesa-reader-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    fn trial(reader: &Reader, trial: Trial) -> Result<Reply, Failure> {
        reader.run(Job::Test(trial), &mut |_| true, &|| false)
    }

    #[test]
    fn reads_files_but_cannot_change_them() {
        let dir = folder("locked");
        let kept = dir.join("kept.txt");
        fs::write(&kept, "the user's words").unwrap();
        let reader = spawning(Role::Indexing, Duration::from_secs(60));

        assert_eq!(trial(&reader, Trial::Read(text(&kept))), Ok(Reply::Done(Ok(()))));
        for refused in [
            Trial::Remove(text(&kept)),
            Trial::Write(text(&kept)),
            Trial::Write(text(&dir.join("new.txt"))),
        ] {
            assert_eq!(
                trial(&reader, refused.clone()),
                Ok(Reply::Done(Err("PermissionDenied".into()))),
                "{refused:?}"
            );
        }
        assert_eq!(fs::read_to_string(&kept).unwrap(), "the user's words");
        assert!(!dir.join("new.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_real_files_in_the_reader() {
        let dir = folder("real");
        let notes = dir.join("notes.txt");
        fs::write(
            &notes,
            "Some notes about the garden, long enough for the model to read them.",
        )
        .unwrap();
        let picture = dir.join("picture.png");
        image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([x as u8 * 2, y as u8 * 3, 90]))
            .save(&picture)
            .unwrap();

        let indexing = spawning(Role::Indexing, Duration::from_secs(60));
        let (mut words, mut units) = (String::new(), Vec::new());
        let item = Item {
            path: notes.clone(),
            words_only: false,
        };
        indexing
            .prepare(
                &item,
                None,
                &mut |unit| {
                    units.push(unit);
                    true
                },
                &mut words,
                &|| false,
            )
            .unwrap();
        assert!(words.contains("garden"), "{words}");
        assert!(
            units
                .iter()
                .any(|unit| matches!(&unit.input, crate::embed::Prepared::Text(text) if text.contains("garden")))
        );

        let mut read = 0;
        let mut ocr = |_: u64, rgb: Vec<u8>, width: u32, height: u32| {
            assert_eq!((rgb.len(), width, height), (96 * 64 * 3, 96, 64));
            read += 1;
        };
        let picture_item = Item {
            path: picture.clone(),
            words_only: false,
        };
        let mut images = 0;
        indexing
            .prepare(
                &picture_item,
                Some(&mut ocr as &mut dyn FnMut(u64, Vec<u8>, u32, u32)),
                &mut |unit| {
                    images += usize::from(matches!(unit.input, crate::embed::Prepared::Image(_)));
                    true
                },
                &mut String::new(),
                &|| false,
            )
            .unwrap();
        assert_eq!((images, read), (1, 1));

        let previews = spawning(Role::Previews, Duration::from_secs(60));
        let png = previews.render(&text(&picture), 256).expect("a preview");
        assert!(image::load_from_memory(&png).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_is_blamed_only_on_the_file_that_caused_it() {
        let dir = folder("crash");
        let (bad, good) = (text(&dir.join("bad.heic")), text(&dir.join("good.heic")));
        let reader = spawning(Role::Indexing, Duration::from_secs(60));
        let (innocent, guilty) = thread::scope(|scope| {
            let innocent = scope.spawn(|| trial(&reader, Trial::Wait(good.clone(), 1500)));
            thread::sleep(Duration::from_millis(300));
            let guilty = trial(&reader, Trial::Crash(bad.clone()));
            (innocent.join().unwrap(), guilty)
        });
        assert_eq!(innocent, Ok(Reply::Done(Ok(()))));
        assert_eq!(
            guilty,
            Err(Failure::File("reading it crashed Mesa's file reader".into()))
        );
        assert_eq!(trial(&reader, Trial::Wait(good, 0)), Ok(Reply::Done(Ok(()))));
        let started = Instant::now();
        assert_eq!(
            trial(&reader, Trial::Crash(bad)),
            Err(Failure::File(BROKE_BEFORE.into()))
        );
        assert!(started.elapsed() < Duration::from_millis(100));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn calls_made_before_a_crash_are_not_made_again() {
        let reader = spawning(Role::Previews, Duration::from_secs(60));
        let mut heard = Vec::new();
        let result = reader.run(
            Job::Test(Trial::CallThenCrash("calls.pdf".into(), 3)),
            &mut |call| {
                if let Call::Passage { locator, .. } = call {
                    heard.push(locator);
                }
                true
            },
            &|| false,
        );
        assert_eq!(
            result,
            Err(Failure::File("reading it crashed Mesa's file reader".into()))
        );
        assert_eq!(heard, [0, 1, 2]);
    }

    #[test]
    fn a_stuck_read_is_stopped_and_blamed() {
        let reader = spawning(Role::Indexing, Duration::from_secs(1));
        let started = Instant::now();
        assert_eq!(
            trial(&reader, Trial::Hang("stuck.pdf".into())),
            Err(Failure::File("reading it got stuck, so Mesa stopped it".into()))
        );
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        assert_eq!(
            trial(&reader, Trial::Wait("next.pdf".into(), 0)),
            Ok(Reply::Done(Ok(())))
        );
    }

    #[test]
    fn stopping_does_not_wait_for_a_stuck_read() {
        let reader = spawning(Role::Indexing, Duration::from_secs(60));
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(300));
                stop.store(true, Ordering::SeqCst);
            });
            reader.run(Job::Test(Trial::Hang("stuck.mp4".into())), &mut |_| true, &|| {
                stop.load(Ordering::SeqCst)
            })
        });
        assert_eq!(result, Err(Failure::File(STOPPED.into())));
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        assert_eq!(
            trial(&reader, Trial::Wait("next.mp4".into(), 0)),
            Ok(Reply::Done(Ok(())))
        );
    }

    #[test]
    fn too_much_memory_stops_the_reader_not_mesa() {
        let reader = spawning(Role::Indexing, Duration::from_secs(60));
        assert_eq!(
            trial(&reader, Trial::Allocate("huge.tif".into())),
            Err(Failure::File("reading it crashed Mesa's file reader".into()))
        );
        assert_eq!(
            trial(&reader, Trial::Wait("next.tif".into(), 0)),
            Ok(Reply::Done(Ok(())))
        );
    }
}
