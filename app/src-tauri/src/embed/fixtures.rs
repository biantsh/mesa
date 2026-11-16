pub fn stereo_wav(rate: u32, seconds: f64) -> Vec<u8> {
    let frames = (f64::from(rate) * seconds) as usize;
    let mut data = Vec::new();
    for n in 0..frames {
        let left = ((2.0 * std::f64::consts::PI * 440.0 * n as f64 / f64::from(rate)).sin() * 20000.0) as i16;
        data.extend_from_slice(&left.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 4).to_le_bytes());
    file.extend_from_slice(&4u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    file
}
