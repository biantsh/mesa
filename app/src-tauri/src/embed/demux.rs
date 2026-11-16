use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use symphonia::core::io::MediaSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Piece {
    at: u64,
    length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Mpeg,
    Adts,
}

impl Codec {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mpeg => "mp3",
            Self::Adts => "aac",
        }
    }
}

pub struct Sound {
    file: BufReader<File>,
    file_at: u64,
    pieces: Vec<Piece>,
    starts: Vec<u64>,
    length: u64,
    position: u64,
}

pub fn sound_of(path: &Path) -> Option<(Sound, Codec)> {
    let mut file = BufReader::with_capacity(1 << 16, File::open(path).ok()?);
    let mut start = [0u8; 12];
    file.read_exact(&mut start).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let (pieces, codec) = if start[..4] == [0, 0, 1, 0xBA] {
        program_stream(&mut file)?
    } else if &start[..4] == b"RIFF" && &start[8..12] == b"AVI " {
        avi(&mut file)?
    } else {
        transport_stream(&mut file)?
    };
    if pieces.is_empty() {
        return None;
    }
    let mut starts = Vec::with_capacity(pieces.len());
    let mut length = 0;
    for piece in &pieces {
        starts.push(length);
        length += u64::from(piece.length);
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    Some((
        Sound {
            file,
            file_at: 0,
            pieces,
            starts,
            length,
            position: 0,
        },
        codec,
    ))
}

fn add(pieces: &mut Vec<Piece>, at: u64, length: u32) {
    if length == 0 {
        return;
    }
    match pieces.last_mut() {
        Some(last) if last.at + u64::from(last.length) == at => last.length += length,
        _ => pieces.push(Piece { at, length }),
    }
}

fn program_stream(file: &mut BufReader<File>) -> Option<(Vec<Piece>, Codec)> {
    let mut pieces = Vec::new();
    let mut stream = None;
    let mut code = [0u8; 4];
    let mut header = [0u8; 258];
    loop {
        if file.read_exact(&mut code).is_err() {
            break;
        }
        if code[..3] != [0, 0, 1] {
            file.seek_relative(-3).ok()?;
            continue;
        }
        match code[3] {
            0xBA => {
                let mut header = [0u8; 10];
                if file.read_exact(&mut header[..1]).is_err() {
                    break;
                }
                if header[0] & 0xC0 == 0x40 {
                    if file.read_exact(&mut header[1..]).is_err() {
                        break;
                    }
                    file.seek_relative(i64::from(header[9] & 7)).ok()?;
                } else {
                    file.seek_relative(7).ok()?;
                }
            }
            0xB9 => break,
            0xBB..=0xFF => {
                let mut length = [0u8; 2];
                if file.read_exact(&mut length).is_err() {
                    break;
                }
                let length = u16::from_be_bytes(length);
                let at = file.stream_position().ok()?;
                let id = code[3];
                if (0xC0..=0xDF).contains(&id) && *stream.get_or_insert(id) == id {
                    let read = usize::from(length).min(header.len());
                    if file.read_exact(&mut header[..read]).is_err() {
                        break;
                    }
                    if let Some(skip) = pes_header_length(&header[..read]) {
                        add(&mut pieces, at + skip as u64, u32::from(length) - skip as u32);
                    }
                    file.seek_relative(i64::from(length) - read as i64).ok()?;
                } else {
                    file.seek_relative(i64::from(length)).ok()?;
                }
            }
            _ => file.seek_relative(-3).ok()?,
        }
    }
    Some((pieces, Codec::Mpeg))
}

fn pes_header_length(rest: &[u8]) -> Option<usize> {
    let length = if rest.first()? & 0xC0 == 0x80 {
        3 + usize::from(*rest.get(2)?)
    } else {
        let mut at = 0;
        while *rest.get(at)? == 0xFF {
            at += 1;
        }
        if rest.get(at)? & 0xC0 == 0x40 {
            at += 2;
        }
        at + match rest.get(at)? & 0xF0 {
            0x20 => 5,
            0x30 => 10,
            _ => 1,
        }
    };
    (length <= rest.len()).then_some(length)
}

fn transport_stream(file: &mut BufReader<File>) -> Option<(Vec<Piece>, Codec)> {
    let mut probe = [0u8; 192 * 3];
    file.read_exact(&mut probe).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let (size, offset) = [(188, 0), (192, 4)]
        .into_iter()
        .find(|&(size, offset)| (0..3).all(|n| probe[n * size + offset] == 0x47))?;
    let mut packet = [0u8; 192];
    let mut maps: Vec<u16> = Vec::new();
    let mut chosen: Option<(u16, Codec)> = None;
    let mut private: Vec<u16> = Vec::new();
    let mut pieces = Vec::new();
    let mut begun = false;
    let mut next = 0u64;
    loop {
        let at = next;
        if file.read_exact(&mut packet[..size]).is_err() {
            break;
        }
        next += size as u64;
        let packet = &packet[offset..size];
        if packet[0] != 0x47 {
            break;
        }
        let pid = u16::from_be_bytes([packet[1], packet[2]]) & 0x1FFF;
        let starts = packet[1] & 0x40 != 0;
        let control = (packet[3] >> 4) & 3;
        let mut payload = 4;
        if control & 2 != 0 {
            payload += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 || payload >= packet.len() {
            continue;
        }
        let body = &packet[payload..];
        if chosen.is_none()
            && starts
            && private.contains(&pid)
            && body.len() > 3
            && body[..3] == [0, 0, 1]
            && (0xC0..=0xDF).contains(&body[3])
        {
            chosen = Some((pid, Codec::Mpeg));
        }
        match chosen {
            Some((sound, _)) if pid == sound => {
                let mut skip = 0;
                if starts {
                    if body.len() < 6 || body[..3] != [0, 0, 1] {
                        continue;
                    }
                    let Some(header) = pes_header_length(&body[6..]) else {
                        continue;
                    };
                    skip = 6 + header;
                    begun = true;
                }
                if begun && skip < body.len() {
                    add(
                        &mut pieces,
                        at + (offset + payload + skip) as u64,
                        (body.len() - skip) as u32,
                    );
                }
            }
            None if pid == 0 && starts => {
                for [program_high, program_low, map_high, map_low] in
                    section(body, 0).unwrap_or_default().as_chunks::<4>().0
                {
                    let program = u16::from_be_bytes([*program_high, *program_low]);
                    let map = u16::from_be_bytes([*map_high, *map_low]) & 0x1FFF;
                    if program != 0 && !maps.contains(&map) {
                        maps.push(map);
                    }
                }
            }
            None if starts && maps.contains(&pid) => {
                let Some(map) = section(body, 2) else { continue };
                if map.len() < 4 {
                    continue;
                }
                let info = usize::from(u16::from_be_bytes([map[2], map[3]]) & 0x0FFF);
                let mut streams = map.get(4 + info..).unwrap_or_default();
                while streams.len() >= 5 {
                    let kind = streams[0];
                    let pid = u16::from_be_bytes([streams[1], streams[2]]) & 0x1FFF;
                    let info = usize::from(u16::from_be_bytes([streams[3], streams[4]]) & 0x0FFF);
                    let codec = match kind {
                        0x03 | 0x04 => Some(Codec::Mpeg),
                        0x0F => Some(Codec::Adts),
                        0x06 => {
                            private.push(pid);
                            None
                        }
                        _ => None,
                    };
                    if let Some(codec) = codec {
                        chosen = Some((pid, codec));
                        break;
                    }
                    streams = streams.get(5 + info..).unwrap_or_default();
                }
            }
            _ => {}
        }
    }
    Some((pieces, chosen?.1))
}

fn section(body: &[u8], id: u8) -> Option<&[u8]> {
    let pointer = usize::from(*body.first()?);
    let table = body.get(1 + pointer..)?;
    if *table.first()? != id || table.len() < 8 {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([table[1], table[2]]) & 0x0FFF);
    table.get(8..(3 + length).checked_sub(4)?)
}

fn avi(file: &mut BufReader<File>) -> Option<(Vec<Piece>, Codec)> {
    let mut sound: Option<u32> = None;
    let mut streams = 0u32;
    let mut last_sound: Option<u32> = None;
    let mut pieces = Vec::new();
    let mut chunk = [0u8; 8];
    let mut wanted = [0u8; 2];
    loop {
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let id = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        match &id {
            b"RIFF" | b"LIST" => {
                file.seek_relative(4).ok()?;
                continue;
            }
            b"strh" => {
                let mut header = vec![0u8; size as usize];
                file.read_exact(&mut header).ok()?;
                last_sound = header.starts_with(b"auds").then_some(streams);
                streams += 1;
            }
            b"strf" => {
                let mut format = vec![0u8; size as usize];
                file.read_exact(&mut format).ok()?;
                let tag = format.get(..2).map(|tag| u16::from_le_bytes([tag[0], tag[1]]));
                if let (None, Some(stream), Some(0x50 | 0x55)) = (sound, last_sound, tag) {
                    sound = Some(stream);
                    wanted = [b'0' + (stream / 10 % 10) as u8, b'0' + (stream % 10) as u8];
                }
            }
            _ => {
                let at = file.stream_position().ok()?;
                if sound.is_some() && id[..2] == wanted && &id[2..] == b"wb" {
                    add(&mut pieces, at, size);
                }
                file.seek_relative(i64::from(size)).ok()?;
            }
        }
        if size % 2 == 1 {
            file.seek_relative(1).ok()?;
        }
    }
    sound?;
    Some((pieces, Codec::Mpeg))
}

impl Read for Sound {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.length || buffer.is_empty() {
            return Ok(0);
        }
        let n = self.starts.partition_point(|&start| start <= self.position) - 1;
        let piece = self.pieces[n];
        let into = self.position - self.starts[n];
        let wanted = buffer.len().min((u64::from(piece.length) - into) as usize);
        let at = piece.at + into;
        if at != self.file_at {
            self.file.seek_relative(at as i64 - self.file_at as i64)?;
        }
        let read = self.file.read(&mut buffer[..wanted])?;
        self.file_at = at + read as u64;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Sound {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let position = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.length.checked_add_signed(by),
            SeekFrom::Current(by) => self.position.checked_add_signed(by),
        };
        self.position = position.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "before the start"))?;
        Ok(self.position)
    }
}

impl MediaSource for Sound {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}
