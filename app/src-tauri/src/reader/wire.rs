use std::io::{self, Read, Write};

use crate::embed::{Prepared, Unit};

const MAGIC: [u8; 4] = *b"MeRd";
const MOST_FRAME: usize = 1 << 30;

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Prepare {
        path: String,
        code: bool,
        words_only: bool,
        read_text: bool,
    },
    Sample {
        path: String,
        code: bool,
        from: u64,
        to: u64,
    },
    Passages {
        path: String,
    },
    Render {
        path: String,
        size: u32,
    },
    RenderPage {
        path: String,
        size: u32,
        page: u32,
    },
    Paging {
        path: String,
    },
    PageTexts {
        path: String,
    },
    Ping,
    #[cfg(test)]
    Test(crate::reader::tests::Trial),
}

impl Job {
    pub fn path(&self) -> Option<&str> {
        match self {
            Job::Prepare { path, .. }
            | Job::Sample { path, .. }
            | Job::Passages { path }
            | Job::Render { path, .. }
            | Job::RenderPage { path, .. }
            | Job::Paging { path }
            | Job::PageTexts { path } => Some(path),
            Job::Ping => None,
            #[cfg(test)]
            Job::Test(trial) => trial.path(),
        }
    }
}

pub enum Call {
    Unit(Unit),
    Text {
        locator: u64,
        rgb: Vec<u8>,
        width: u32,
        height: u32,
    },
    Passage {
        locator: u64,
        text: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum Reply {
    Words(Result<String, String>),
    Slots(Result<u64, String>),
    Done(Result<(), String>),
    Bytes(Option<Vec<u8>>),
    Paging(Option<(u64, Option<String>)>),
    Texts(Option<Vec<String>>),
}

pub enum ToHelper {
    Start(u64, Job),
    Answer(u64, bool),
    Cancel(u64),
}

pub enum FromHelper {
    Hello,
    Call(u64, Call),
    Done(u64, Reply),
}

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }

    fn bool(&mut self, value: bool) -> &mut Self {
        self.u8(value as u8)
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn u64(&mut self, value: u64) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.u64(value.len() as u64);
        self.0.extend_from_slice(value);
        self
    }

    fn str(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    fn text_result<T>(&mut self, value: &Result<T, String>, ok: impl FnOnce(&mut Self, &T)) -> &mut Self {
        match value {
            Ok(value) => {
                self.u8(0);
                ok(self, value);
            }
            Err(why) => {
                self.u8(1).str(why);
            }
        }
        self
    }

    fn option<T>(&mut self, value: &Option<T>, some: impl FnOnce(&mut Self, &T)) -> &mut Self {
        match value {
            Some(value) => {
                self.u8(1);
                some(self, value);
            }
            None => {
                self.u8(0);
            }
        }
        self
    }
}

struct In<'a>(&'a [u8]);

impl In<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        if count > self.0.len() {
            return None;
        }
        let (taken, rest) = self.0.split_at(count);
        self.0 = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn bytes(&mut self) -> Option<Vec<u8>> {
        let count = usize::try_from(self.u64()?).ok()?;
        Some(self.take(count)?.to_vec())
    }

    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }

    fn text_result<T>(&mut self, ok: impl FnOnce(&mut Self) -> Option<T>) -> Option<Result<T, String>> {
        match self.u8()? {
            0 => Some(Ok(ok(self)?)),
            1 => Some(Err(self.str()?)),
            _ => None,
        }
    }

    fn option<T>(&mut self, some: impl FnOnce(&mut Self) -> Option<T>) -> Option<Option<T>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(some(self)?)),
            _ => None,
        }
    }

    fn end<T>(&self, value: T) -> Option<T> {
        self.0.is_empty().then_some(value)
    }
}

impl Job {
    fn put(&self, out: &mut Out) {
        match self {
            Job::Prepare {
                path,
                code,
                words_only,
                read_text,
            } => {
                out.u8(0).str(path).bool(*code).bool(*words_only).bool(*read_text);
            }
            Job::Sample { path, code, from, to } => {
                out.u8(1).str(path).bool(*code).u64(*from).u64(*to);
            }
            Job::Passages { path } => {
                out.u8(2).str(path);
            }
            Job::Render { path, size } => {
                out.u8(3).str(path).u32(*size);
            }
            Job::RenderPage { path, size, page } => {
                out.u8(4).str(path).u32(*size).u32(*page);
            }
            Job::Paging { path } => {
                out.u8(5).str(path);
            }
            Job::PageTexts { path } => {
                out.u8(6).str(path);
            }
            Job::Ping => {
                out.u8(7);
            }
            #[cfg(test)]
            Job::Test(trial) => {
                out.u8(8).str(&trial.to_line());
            }
        }
    }

    fn get(input: &mut In) -> Option<Self> {
        Some(match input.u8()? {
            0 => Job::Prepare {
                path: input.str()?,
                code: input.bool()?,
                words_only: input.bool()?,
                read_text: input.bool()?,
            },
            1 => Job::Sample {
                path: input.str()?,
                code: input.bool()?,
                from: input.u64()?,
                to: input.u64()?,
            },
            2 => Job::Passages { path: input.str()? },
            3 => Job::Render {
                path: input.str()?,
                size: input.u32()?,
            },
            4 => Job::RenderPage {
                path: input.str()?,
                size: input.u32()?,
                page: input.u32()?,
            },
            5 => Job::Paging { path: input.str()? },
            6 => Job::PageTexts { path: input.str()? },
            7 => Job::Ping,
            #[cfg(test)]
            8 => Job::Test(crate::reader::tests::Trial::from_line(&input.str()?)?),
            _ => return None,
        })
    }
}

impl Call {
    fn put(&self, out: &mut Out) {
        match self {
            Call::Unit(unit) => {
                out.u8(0).u64(unit.locator);
                match &unit.input {
                    Prepared::Image(bytes) => out.u8(0).bytes(bytes),
                    Prepared::Audio(bytes) => out.u8(1).bytes(bytes),
                    Prepared::Text(text) => out.u8(2).str(text),
                };
            }
            Call::Text {
                locator,
                rgb,
                width,
                height,
            } => {
                out.u8(1).u64(*locator).bytes(rgb).u32(*width).u32(*height);
            }
            Call::Passage { locator, text } => {
                out.u8(2).u64(*locator).str(text);
            }
        }
    }

    fn get(input: &mut In) -> Option<Self> {
        Some(match input.u8()? {
            0 => {
                let locator = input.u64()?;
                let input = match input.u8()? {
                    0 => Prepared::Image(input.bytes()?),
                    1 => Prepared::Audio(input.bytes()?),
                    2 => Prepared::Text(input.str()?),
                    _ => return None,
                };
                Call::Unit(Unit { locator, input })
            }
            1 => Call::Text {
                locator: input.u64()?,
                rgb: input.bytes()?,
                width: input.u32()?,
                height: input.u32()?,
            },
            2 => Call::Passage {
                locator: input.u64()?,
                text: input.str()?,
            },
            _ => return None,
        })
    }
}

impl Reply {
    fn put(&self, out: &mut Out) {
        match self {
            Reply::Words(words) => {
                out.u8(0).text_result(words, |out, words| {
                    out.str(words);
                });
            }
            Reply::Slots(slots) => {
                out.u8(1).text_result(slots, |out, slots| {
                    out.u64(*slots);
                });
            }
            Reply::Done(done) => {
                out.u8(2).text_result(done, |_, ()| {});
            }
            Reply::Bytes(bytes) => {
                out.u8(3).option(bytes, |out, bytes| {
                    out.bytes(bytes);
                });
            }
            Reply::Paging(paging) => {
                out.u8(4).option(paging, |out, (pages, note)| {
                    out.u64(*pages).option(note, |out, note| {
                        out.str(note);
                    });
                });
            }
            Reply::Texts(texts) => {
                out.u8(5).option(texts, |out, texts| {
                    out.u64(texts.len() as u64);
                    for text in texts {
                        out.str(text);
                    }
                });
            }
        }
    }

    fn get(input: &mut In) -> Option<Self> {
        Some(match input.u8()? {
            0 => Reply::Words(input.text_result(In::str)?),
            1 => Reply::Slots(input.text_result(In::u64)?),
            2 => Reply::Done(input.text_result(|_| Some(()))?),
            3 => Reply::Bytes(input.option(In::bytes)?),
            4 => Reply::Paging(input.option(|input| Some((input.u64()?, input.option(In::str)?)))?),
            5 => Reply::Texts(input.option(|input| {
                let count = input.u64()?;
                let mut texts = Vec::new();
                for _ in 0..count {
                    texts.push(input.str()?);
                }
                Some(texts)
            })?),
            _ => return None,
        })
    }
}

impl ToHelper {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            ToHelper::Start(id, job) => {
                out.u8(0).u64(*id);
                job.put(&mut out);
            }
            ToHelper::Answer(id, more) => {
                out.u8(1).u64(*id).bool(*more);
            }
            ToHelper::Cancel(id) => {
                out.u8(2).u64(*id);
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut input = In(bytes);
        let message = match input.u8()? {
            0 => ToHelper::Start(input.u64()?, Job::get(&mut input)?),
            1 => ToHelper::Answer(input.u64()?, input.bool()?),
            2 => ToHelper::Cancel(input.u64()?),
            _ => return None,
        };
        input.end(message)
    }
}

impl FromHelper {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            FromHelper::Hello => {
                out.u8(0);
            }
            FromHelper::Call(id, call) => {
                out.u8(1).u64(*id);
                call.put(&mut out);
            }
            FromHelper::Done(id, reply) => {
                out.u8(2).u64(*id);
                reply.put(&mut out);
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut input = In(bytes);
        let message = match input.u8()? {
            0 => FromHelper::Hello,
            1 => FromHelper::Call(input.u64()?, Call::get(&mut input)?),
            2 => FromHelper::Done(input.u64()?, Reply::get(&mut input)?),
            _ => return None,
        };
        input.end(message)
    }
}

pub fn write_frame(to: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let length = u32::try_from(body.len())
        .ok()
        .filter(|&length| length as usize <= MOST_FRAME)
        .ok_or_else(|| io::Error::other("the message is too big to send"))?;
    let mut head = [0u8; 8];
    head[..4].copy_from_slice(&MAGIC);
    head[4..].copy_from_slice(&length.to_le_bytes());
    to.write_all(&head)?;
    to.write_all(body)?;
    to.flush()
}

pub fn read_frame(from: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut head = [0u8; 8];
    match from.read_exact(&mut head[..1]) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        other => other?,
    }
    from.read_exact(&mut head[1..])?;
    if head[..4] != MAGIC {
        return Err(io::Error::other("the reader sent something that isn't a message"));
    }
    body(from, &head)
}

pub fn read_first_frame(from: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut head = [0u8; 8];
    let mut seen = 0;
    while seen < 4 {
        let mut byte = [0u8];
        if from.read(&mut byte)? == 0 {
            return Ok(None);
        }
        if byte[0] == MAGIC[seen] {
            seen += 1;
        } else {
            seen = usize::from(byte[0] == MAGIC[0]);
        }
    }
    head[..4].copy_from_slice(&MAGIC);
    from.read_exact(&mut head[4..])?;
    body(from, &head)
}

fn body(from: &mut impl Read, head: &[u8; 8]) -> io::Result<Option<Vec<u8>>> {
    let length = u32::from_le_bytes(head[4..].try_into().expect("four bytes")) as usize;
    if length > MOST_FRAME {
        return Err(io::Error::other("the reader sent a message too big to take"));
    }
    let mut body = Vec::new();
    from.take(length as u64).read_to_end(&mut body)?;
    if body.len() != length {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_to(message: ToHelper) -> ToHelper {
        ToHelper::decode(&message.encode()).expect("decodes")
    }

    fn round_trip_from(message: FromHelper) -> FromHelper {
        FromHelper::decode(&message.encode()).expect("decodes")
    }

    #[test]
    fn messages_come_back_as_they_went() {
        let jobs = [
            Job::Prepare {
                path: "C:\\Users\\a\\b.pdf".into(),
                code: false,
                words_only: true,
                read_text: true,
            },
            Job::Sample {
                path: "/home/a/song.mp3".into(),
                code: true,
                from: 2,
                to: 8,
            },
            Job::Passages {
                path: "研究.docx".into(),
            },
            Job::Render {
                path: "a.png".into(),
                size: 256,
            },
            Job::RenderPage {
                path: "a.pptx".into(),
                size: 1600,
                page: 3,
            },
            Job::Paging { path: "a.pdf".into() },
            Job::PageTexts { path: "a.odt".into() },
            Job::Ping,
        ];
        for job in jobs {
            let ToHelper::Start(7, back) = round_trip_to(ToHelper::Start(7, job.clone())) else {
                panic!("not a start")
            };
            assert_eq!(back, job);
        }
        assert!(matches!(
            round_trip_to(ToHelper::Answer(9, false)),
            ToHelper::Answer(9, false)
        ));
        assert!(matches!(
            round_trip_to(ToHelper::Cancel(u64::MAX)),
            ToHelper::Cancel(u64::MAX)
        ));

        let replies = [
            Reply::Words(Ok("some words".into())),
            Reply::Words(Err("it is locked".into())),
            Reply::Slots(Ok(8)),
            Reply::Done(Ok(())),
            Reply::Done(Err("no".into())),
            Reply::Bytes(Some(vec![1, 2, 3])),
            Reply::Bytes(None),
            Reply::Paging(Some((12, Some("a note".into())))),
            Reply::Paging(Some((1, None))),
            Reply::Paging(None),
            Reply::Texts(Some(vec!["one".into(), String::new(), "three".into()])),
            Reply::Texts(None),
        ];
        for reply in replies {
            let encoded = FromHelper::Done(3, reply).encode();
            let Some(FromHelper::Done(3, back)) = FromHelper::decode(&encoded) else {
                panic!("not a reply")
            };
            assert_eq!(FromHelper::Done(3, back).encode(), encoded);
        }

        let calls = [
            Call::Unit(Unit {
                locator: 4,
                input: Prepared::Image(vec![9; 1000]),
            }),
            Call::Unit(Unit {
                locator: 0,
                input: Prepared::Audio(vec![]),
            }),
            Call::Unit(Unit {
                locator: 1,
                input: Prepared::Text("passage".into()),
            }),
            Call::Text {
                locator: 2,
                rgb: vec![1, 2, 3],
                width: 1,
                height: 1,
            },
            Call::Passage {
                locator: 5,
                text: "text".into(),
            },
        ];
        for call in calls {
            let encoded = FromHelper::Call(1, call).encode();
            let Some(FromHelper::Call(1, back)) = FromHelper::decode(&encoded) else {
                panic!("not a call")
            };
            assert_eq!(FromHelper::Call(1, back).encode(), encoded);
        }
        assert!(matches!(round_trip_from(FromHelper::Hello), FromHelper::Hello));
    }

    #[test]
    fn broken_messages_are_refused() {
        let good = FromHelper::Done(3, Reply::Texts(Some(vec!["one".into()]))).encode();
        for cut in 0..good.len() {
            assert!(FromHelper::decode(&good[..cut]).is_none(), "accepted {cut} bytes");
        }
        let mut longer = good.clone();
        longer.push(0);
        assert!(FromHelper::decode(&longer).is_none());
        let mut huge = FromHelper::Call(
            1,
            Call::Passage {
                locator: 0,
                text: String::new(),
            },
        )
        .encode();
        let at = huge.len() - 8;
        huge[at..].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(FromHelper::decode(&huge).is_none());
    }

    #[test]
    fn frames_survive_the_pipe_and_noise_before_the_first() {
        let mut stream = b"running 1 test\nMeR".to_vec();
        write_frame(&mut stream, &FromHelper::Hello.encode()).unwrap();
        write_frame(&mut stream, &[1, 2, 3]).unwrap();
        let mut reading = stream.as_slice();
        assert_eq!(
            read_first_frame(&mut reading).unwrap(),
            Some(FromHelper::Hello.encode())
        );
        assert_eq!(read_frame(&mut reading).unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(read_frame(&mut reading).unwrap(), None);

        let mut cut = Vec::new();
        write_frame(&mut cut, &[1, 2, 3]).unwrap();
        cut.pop();
        assert!(read_frame(&mut cut.as_slice()).is_err());
        assert!(read_frame(&mut b"nonsense".as_slice()).is_err());
    }
}
