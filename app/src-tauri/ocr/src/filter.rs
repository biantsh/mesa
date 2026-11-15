use super::{
    geometry::Picture,
    net::Net,
    resize::{gray, resize_gray},
};

const HEIGHT: usize = 48;
const MAX_WIDTH: usize = 1024;
const ROUND_TO: usize = 64;
const LABELS: [&str; 2] = ["read", "skip"];

pub fn check(net: &Net) -> Result<f32, String> {
    let Some(labels) = net.metadata("labels") else {
        return Err("the line filter doesn't say what its outputs are".into());
    };
    let labels: Vec<String> =
        serde_json::from_str(labels).map_err(|e| format!("the line filter's outputs can't be read: {e}"))?;
    if labels != LABELS {
        return Err(format!("the line filter's outputs are {labels:?}, not {LABELS:?}"));
    }
    let skip_at = net.metadata("skip_at").and_then(|at| at.parse::<f32>().ok());
    skip_at
        .filter(|at| (0.0..=1.0).contains(at))
        .ok_or_else(|| "the line filter doesn't say when to leave a line out".into())
}

fn to_line(line: Picture) -> Result<(Vec<u8>, usize), String> {
    let width = ((line.width as f64 * HEIGHT as f64 / line.height.max(1) as f64).round() as usize).clamp(8, MAX_WIDTH);
    Ok((
        resize_gray(&gray(line), (line.width, line.height), (width, HEIGHT))?,
        width,
    ))
}

pub fn skip_likelihood(net: &Net, line: Picture) -> Result<f32, String> {
    let (gray, width) = to_line(line)?;
    let padded = width.next_multiple_of(ROUND_TO);
    let mut ends: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| [0, 1, width - 2, width - 1].map(|x| gray[y * width + x]))
        .collect();
    ends.sort_unstable();
    let fill = f32::from(ends[ends.len() / 2]) / 127.5 - 1.0;
    let at = (padded - width) / 2;
    let mut input = vec![fill; HEIGHT * padded];
    for y in 0..HEIGHT {
        for x in 0..width {
            input[y * padded + at + x] = f32::from(gray[y * width + x]) / 127.5 - 1.0;
        }
    }
    let (_, logits) = net
        .run(&[1, 1, HEIGHT, padded], input)?
        .into_iter()
        .next()
        .ok_or("the line filter gave nothing")?;
    let [read, skip] = logits[..] else {
        return Err(format!(
            "the line filter gave {} numbers, not {}",
            logits.len(),
            LABELS.len()
        ));
    };
    Ok(1.0 / (1.0 + (read - skip).exp()))
}
