use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use half::f16;

pub const DIMS: usize = 256;
const ELEMENT_BYTES: usize = 2;
pub const RECORD_BYTES: usize = DIMS * ELEMENT_BYTES;
const HEADER_BYTES: u64 = 64;

const MAGIC: &[u8; 8] = b"MESAVEC\0";
const VERSION: u32 = 1;
const F16: u32 = 1;
pub type Record = [u8; RECORD_BYTES];
const RUN_SLOTS: u64 = 4096;
const MAX_GAP_SLOTS: u64 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub locator: u64,
    pub record: Record,
}

pub struct VectorFile {
    file: File,
    next_slot: u64,
}

impl VectorFile {
    pub fn open(path: &Path, config_id: u32) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let expected = header(config_id);
        if file.metadata()?.len() == 0 {
            file.write_all(&expected)?;
        } else {
            let mut found = [0u8; HEADER_BYTES as usize];
            file.read_exact(&mut found)
                .map_err(|_| invalid("the vector file is truncated"))?;
            if found != expected {
                return Err(invalid("the vector file was written for a different format or model"));
            }
        }
        let len = file.metadata()?.len();
        Ok(Self {
            file,
            next_slot: (len - HEADER_BYTES) / RECORD_BYTES as u64,
        })
    }

    pub fn next_slot(&self) -> u64 {
        self.next_slot
    }

    pub fn truncate_to(&mut self, slots: u64) -> io::Result<()> {
        if slots > self.next_slot {
            return Err(invalid("the vector file is shorter than the database expects"));
        }
        if slots < self.next_slot {
            self.file.set_len(HEADER_BYTES + slots * RECORD_BYTES as u64)?;
            self.next_slot = slots;
        }
        Ok(())
    }

    pub fn write(&mut self, writes: &mut [(u64, Record)]) -> io::Result<()> {
        writes.sort_unstable_by_key(|(slot, _)| *slot);
        let mut i = 0;
        while i < writes.len() {
            let start = writes[i].0;
            let mut run = Vec::new();
            let mut slot = start;
            while i < writes.len() && writes[i].0 == slot {
                run.extend_from_slice(&writes[i].1);
                slot += 1;
                i += 1;
            }
            self.file
                .seek(SeekFrom::Start(HEADER_BYTES + start * RECORD_BYTES as u64))?;
            self.file.write_all(&run)?;
            self.next_slot = self.next_slot.max(slot);
        }
        Ok(())
    }

    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_data()
    }

    pub fn read_each(&mut self, slots: &[u64], visit: &mut dyn FnMut(u64, &Record)) -> io::Result<()> {
        debug_assert!(
            slots.windows(2).all(|pair| pair[0] < pair[1]),
            "the slots are in ascending order"
        );
        let mut buffer = Vec::new();
        let mut start = 0;
        while start < slots.len() {
            let first = slots[start];
            let mut end = start + 1;
            while end < slots.len() && slots[end] - slots[end - 1] <= MAX_GAP_SLOTS && slots[end] - first < RUN_SLOTS {
                end += 1;
            }
            let count = (slots[end - 1] - first + 1) as usize;
            buffer.resize(count * RECORD_BYTES, 0);
            self.file
                .seek(SeekFrom::Start(HEADER_BYTES + first * RECORD_BYTES as u64))?;
            self.file.read_exact(&mut buffer)?;
            for &slot in &slots[start..end] {
                let at = (slot - first) as usize * RECORD_BYTES;
                let record = <&Record>::try_from(&buffer[at..at + RECORD_BYTES]).expect("a record is this long");
                visit(slot, record);
            }
            start = end;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn read(&mut self, slot: u64) -> io::Result<Vec<f16>> {
        let mut bytes = [0u8; RECORD_BYTES];
        self.file
            .seek(SeekFrom::Start(HEADER_BYTES + slot * RECORD_BYTES as u64))?;
        self.file.read_exact(&mut bytes)?;
        Ok(bytes
            .chunks_exact(ELEMENT_BYTES)
            .map(|b| f16::from_le_bytes([b[0], b[1]]))
            .collect())
    }
}

fn header(config_id: u32) -> [u8; HEADER_BYTES as usize] {
    let mut bytes = [0u8; HEADER_BYTES as usize];
    bytes[0..8].copy_from_slice(MAGIC);
    bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&config_id.to_le_bytes());
    bytes[16..20].copy_from_slice(&(DIMS as u32).to_le_bytes());
    bytes[20..24].copy_from_slice(&F16.to_le_bytes());
    bytes
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn record_from(output: &[f32]) -> Option<Record> {
    let prefix = output.get(..DIMS)?;
    let length = prefix.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let mut record = [0u8; RECORD_BYTES];
    for (v, out) in prefix.iter().zip(record.chunks_exact_mut(ELEMENT_BYTES)) {
        out.copy_from_slice(&f16::from_f32(v / length).to_le_bytes());
    }
    Some(record)
}

pub fn to_floats(record: &Record) -> [f32; DIMS] {
    let mut values = [0f32; DIMS];
    for (value, bytes) in values.iter_mut().zip(record.chunks_exact(ELEMENT_BYTES)) {
        *value = f16::from_le_bytes([bytes[0], bytes[1]]).to_f32();
    }
    values
}

pub fn blend(parts: &[(Record, f32)]) -> Option<Record> {
    let mut sum = [0f32; DIMS];
    for (record, weight) in parts {
        for (total, value) in sum.iter_mut().zip(to_floats(record)) {
            *total += value * weight;
        }
    }
    record_from(&sum)
}

pub fn similarity(query: &[f32; DIMS], record: &Record) -> f32 {
    query
        .iter()
        .zip(record.chunks_exact(ELEMENT_BYTES))
        .map(|(q, bytes)| q * f16::from_le_bytes([bytes[0], bytes[1]]).to_f32())
        .sum()
}

#[cfg(test)]
pub fn random_record() -> Record {
    let mut values = [0f32; DIMS];
    for v in &mut values {
        *v = fastrand::f32() * 2.0 - 1.0;
    }
    let length = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    let mut record = [0u8; RECORD_BYTES];
    for (v, out) in values.iter().zip(record.chunks_exact_mut(ELEMENT_BYTES)) {
        out.copy_from_slice(&f16::from_f32(v / length).to_le_bytes());
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-vectors-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("vectors.bin")
    }

    fn filled(byte: u8) -> Record {
        [byte; RECORD_BYTES]
    }

    fn values(record: &Record) -> Vec<f32> {
        record
            .chunks_exact(2)
            .map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32())
            .collect()
    }

    #[test]
    fn output_truncated_and_normalized() {
        let output: Vec<f32> = (0..768).map(|i| if i < DIMS { 3.0 } else { 1000.0 }).collect();
        let kept = values(&record_from(&output).unwrap());
        assert_eq!(kept.len(), DIMS);
        let length = kept.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 0.001, "length was {length}");
        assert!((kept[0] - 1.0 / (DIMS as f32).sqrt()).abs() < 0.001);
    }

    #[test]
    fn slots_survive_reopen() {
        let path = temp_file("roundtrip");
        let mut file = VectorFile::open(&path, 1).unwrap();
        let one = f16::from_f32(1.0).to_le_bytes();
        let mut first = [0u8; RECORD_BYTES];
        first[..2].copy_from_slice(&one);
        file.write(&mut [(5, filled(0)), (0, first), (1, filled(0))]).unwrap();
        file.sync().unwrap();
        assert_eq!(file.next_slot(), 6);
        drop(file);

        let mut file = VectorFile::open(&path, 1).unwrap();
        assert_eq!(file.next_slot(), 6);
        assert_eq!(file.read(0).unwrap()[0], f16::from_f32(1.0));
        assert_eq!(file.read(5).unwrap()[0], f16::from_f32(0.0));
    }

    #[test]
    fn rejects_other_config() {
        let path = temp_file("config");
        drop(VectorFile::open(&path, 1).unwrap());
        assert!(VectorFile::open(&path, 2).is_err());
    }

    #[test]
    fn truncate_never_extends() {
        let path = temp_file("truncate");
        let mut file = VectorFile::open(&path, 1).unwrap();
        file.write(&mut [(0, filled(1)), (1, filled(2)), (2, filled(3))])
            .unwrap();
        file.truncate_to(1).unwrap();
        assert_eq!(file.next_slot(), 1);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            HEADER_BYTES + RECORD_BYTES as u64
        );
        assert!(file.truncate_to(2).is_err());
    }
}
