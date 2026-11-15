use super::{
    geometry::{Picture, Point, Rect, convex_hull, in_reading_order, min_area_rect},
    net::Net,
    resize::resize_rgb,
};

const MAX_SIDE: usize = 2000;
const FIRST_SIDE: usize = 768;
const MIN_SIDE: usize = 736;
const THRESHOLD: f32 = 0.3;
const BOX_THRESHOLD: f32 = 0.5;
const UNCLIP_RATIO: f32 = 1.6;
const MIN_SIZE: f32 = 3.0;
const MAX_CANDIDATES: usize = 1000;
const ROUND_TO: usize = 128;
const SAME_LINE: f32 = 10.0;
pub type Found = [Point; 4];

fn round32(x: f64) -> usize {
    ((x / 32.0).round_ties_even() as usize * 32).max(32)
}

pub fn input_size(width: usize, height: usize, max_side: usize) -> (usize, usize) {
    let (mut w, mut h) = (width as f64, height as f64);
    if w.max(h) > max_side as f64 {
        let ratio = max_side as f64 / w.max(h);
        (w, h) = (round32((w * ratio).trunc()) as f64, round32((h * ratio).trunc()) as f64);
    }
    let ratio = if w.min(h) < MIN_SIDE as f64 {
        (MIN_SIDE as f64 / w.min(h)).min((max_side as f64 / w.max(h)).max(1.0))
    } else {
        1.0
    };
    (round32((w * ratio).trunc()), round32((h * ratio).trunc()))
}

pub fn first_look() -> [usize; 4] {
    [1, 3, FIRST_SIDE, FIRST_SIDE]
}

pub fn common_sizes() -> Vec<[usize; 4]> {
    let longest = MAX_SIDE.next_multiple_of(ROUND_TO);
    let mut sizes = vec![first_look()];
    for short in (3 * ROUND_TO..=longest).step_by(ROUND_TO) {
        sizes.push([1, 3, short, longest]);
        if short != longest {
            sizes.push([1, 3, longest, short]);
        }
    }
    sizes
}

pub fn lines(net: &Net, picture: Picture) -> Result<Vec<Found>, String> {
    if picture.width < 4 || picture.height < 4 {
        return Ok(Vec::new());
    }
    let full = input_size(picture.width, picture.height, MAX_SIDE);
    let first = input_size(picture.width, picture.height, FIRST_SIDE);
    if first == full {
        return lines_at(net, picture, full, false);
    }
    if lines_at(net, picture, first, true)?.is_empty() {
        return Ok(Vec::new());
    }
    lines_at(net, picture, full, false)
}

fn lines_at(net: &Net, picture: Picture, (w, h): (usize, usize), first: bool) -> Result<Vec<Found>, String> {
    let (pw, ph) = if first {
        (FIRST_SIDE, FIRST_SIDE)
    } else {
        (w.next_multiple_of(ROUND_TO), h.next_multiple_of(ROUND_TO))
    };
    let scaled = resize_rgb(picture, w, h)?;
    let mut input = vec![0f32; 3 * ph * pw];
    for y in 0..h {
        for x in 0..w {
            let pixel = &scaled[(y * w + x) * 3..][..3];
            for (channel, &value) in [pixel[2], pixel[1], pixel[0]].iter().enumerate() {
                input[(channel * ph + y) * pw + x] = f32::from(value) / 127.5 - 1.0;
            }
        }
    }
    let (shape, map) = net
        .run(&[1, 3, ph, pw], input)?
        .into_iter()
        .next()
        .ok_or("the detector gave nothing")?;
    if shape.len() != 4 || shape[2] != ph || shape[3] != pw {
        return Err(format!("the detector gave a map of {shape:?} for {ph} x {pw}"));
    }
    let (sx, sy) = (picture.width as f32 / w as f32, picture.height as f32 / h as f32);
    let mut found = boxes(&map, pw, w, h)
        .into_iter()
        .filter_map(|rect| {
            let corners = rect.corners.map(|[x, y]| {
                [
                    (x * sx).round().clamp(0.0, picture.width as f32 - 1.0).trunc(),
                    (y * sy).round().clamp(0.0, picture.height as f32 - 1.0).trunc(),
                ]
            });
            let corners = in_reading_order(corners);
            let side = |a: Point, b: Point| (a[0] - b[0]).hypot(a[1] - b[1]) as usize;
            (side(corners[0], corners[1]) > 3 && side(corners[0], corners[3]) > 3).then_some(corners)
        })
        .collect::<Vec<_>>();
    sort_for_reading(&mut found);
    Ok(found)
}

fn boxes(map: &[f32], stride: usize, width: usize, height: usize) -> Vec<Rect> {
    let text = |x: usize, y: usize| map[y * stride + x] > THRESHOLD;
    let mut mask = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            mask[y * width + x] = text(x, y)
                || (x > 0 && text(x - 1, y))
                || (y > 0 && text(x, y - 1))
                || (x > 0 && y > 0 && text(x - 1, y - 1));
        }
    }
    let mut out = Vec::new();
    for blob in blobs(&mut mask, width, height).into_iter().take(MAX_CANDIDATES) {
        let rect = min_area_rect(&convex_hull(blob));
        if rect.short_side() < MIN_SIZE {
            continue;
        }
        let score = mean_inside(map, stride, width, height, rect.corners);
        if score < BOX_THRESHOLD {
            continue;
        }
        let area = rect.width * rect.height;
        let perimeter = 2.0 * (rect.width + rect.height);
        let grown = rect.grown(area * UNCLIP_RATIO / perimeter);
        if grown.short_side() < MIN_SIZE + 2.0 {
            continue;
        }
        out.push(grown);
    }
    out
}

fn blobs(mask: &mut [bool], width: usize, height: usize) -> Vec<Vec<[i32; 2]>> {
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] {
            continue;
        }
        mask[start] = false;
        stack.push(start);
        let mut rows: std::collections::HashMap<usize, (usize, usize)> = Default::default();
        while let Some(at) = stack.pop() {
            let (x, y) = (at % width, at / width);
            let row = rows.entry(y).or_insert((x, x));
            *row = (row.0.min(x), row.1.max(x));
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let next = ny as usize * width + nx as usize;
                    if mask[next] {
                        mask[next] = false;
                        stack.push(next);
                    }
                }
            }
        }
        out.push(
            rows.into_iter()
                .flat_map(|(y, (a, b))| [[a as i32, y as i32], [b as i32, y as i32]])
                .collect(),
        );
    }
    out
}

fn mean_inside(map: &[f32], stride: usize, width: usize, height: usize, corners: [Point; 4]) -> f32 {
    let clamp = |v: f32, hi: usize| (v.max(0.0) as usize).min(hi - 1);
    let xs = corners.map(|c| c[0]);
    let ys = corners.map(|c| c[1]);
    let (x0, x1) = (
        clamp(xs.iter().copied().fold(f32::MAX, f32::min).floor(), width),
        clamp(xs.iter().copied().fold(f32::MIN, f32::max).ceil(), width),
    );
    let (y0, y1) = (
        clamp(ys.iter().copied().fold(f32::MAX, f32::min).floor(), height),
        clamp(ys.iter().copied().fold(f32::MIN, f32::max).ceil(), height),
    );
    let polygon = corners.map(|c| [c[0].trunc() as i64, c[1].trunc() as i64]);
    let inside = |x: i64, y: i64| {
        let mut sign = 0;
        for i in 0..4 {
            let (a, b) = (polygon[i], polygon[(i + 1) % 4]);
            let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            if cross != 0 {
                if sign != 0 && cross.signum() != sign {
                    return false;
                }
                sign = cross.signum();
            }
        }
        true
    };
    let (mut sum, mut count) = (0f64, 0usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if inside(x as i64, y as i64) {
                sum += f64::from(map[y * stride + x]);
                count += 1;
            }
        }
    }
    if count == 0 { 0.0 } else { (sum / count as f64) as f32 }
}

fn sort_for_reading(found: &mut [Found]) {
    found.sort_by(|a, b| a[0][1].total_cmp(&b[0][1]));
    let mut line = 0;
    let mut lines = Vec::with_capacity(found.len());
    for i in 0..found.len() {
        if i > 0 && found[i][0][1] - found[i - 1][0][1] >= SAME_LINE {
            line += 1;
        }
        lines.push(line);
    }
    let mut order: Vec<usize> = (0..found.len()).collect();
    order.sort_by(|&a, &b| lines[a].cmp(&lines[b]).then(found[a][0][0].total_cmp(&found[b][0][0])));
    let sorted: Vec<Found> = order.iter().map(|&i| found[i]).collect();
    found.clone_from_slice(&sorted);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_sorted_in_reading_order() {
        let at = |x: f32, y: f32| [[x, y], [x + 10.0, y], [x + 10.0, y + 5.0], [x, y + 5.0]];
        let mut found = vec![at(50.0, 31.0), at(0.0, 100.0), at(80.0, 25.0), at(10.0, 28.0)];
        sort_for_reading(&mut found);
        let xs: Vec<f32> = found.iter().map(|f| f[0][0]).collect();
        assert_eq!(xs, [10.0, 50.0, 80.0, 0.0]);
    }
}
