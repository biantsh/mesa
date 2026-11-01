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
