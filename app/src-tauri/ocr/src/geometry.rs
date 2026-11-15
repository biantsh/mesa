pub type Point = [f32; 2];

pub fn convex_hull(mut points: Vec<[i32; 2]>) -> Vec<[i32; 2]> {
    points.sort_unstable();
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: [i32; 2], a: [i32; 2], b: [i32; 2]| {
        i64::from(a[0] - o[0]) * i64::from(b[1] - o[1]) - i64::from(a[1] - o[1]) * i64::from(b[0] - o[0])
    };
    let mut hull: Vec<[i32; 2]> = Vec::with_capacity(points.len() + 1);
    for pass in [&points[..], &points.iter().rev().copied().collect::<Vec<_>>()[..]] {
        let start = hull.len();
        for &p in pass {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub corners: [Point; 4],
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn short_side(&self) -> f32 {
        self.width.min(self.height)
    }

    pub fn grown(&self, by: f32) -> Rect {
        let centre = [
            self.corners.iter().map(|c| c[0]).sum::<f32>() / 4.0,
            self.corners.iter().map(|c| c[1]).sum::<f32>() / 4.0,
        ];
        let (along, across) = (
            unit(self.corners[0], self.corners[1]),
            unit(self.corners[1], self.corners[2]),
        );
        let (half_w, half_h) = (self.width / 2.0 + by, self.height / 2.0 + by);
        let corner = |a: f32, b: f32| {
            [
                centre[0] + a * along[0] + b * across[0],
                centre[1] + a * along[1] + b * across[1],
            ]
        };
        Rect {
            corners: [
                corner(-half_w, -half_h),
                corner(half_w, -half_h),
                corner(half_w, half_h),
                corner(-half_w, half_h),
            ],
            width: self.width + 2.0 * by,
            height: self.height + 2.0 * by,
        }
    }
}

fn unit(from: Point, to: Point) -> Point {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let length = dx.hypot(dy);
    if length == 0.0 {
        [1.0, 0.0]
    } else {
        [dx / length, dy / length]
    }
}

pub fn min_area_rect(hull: &[[i32; 2]]) -> Rect {
    let points: Vec<Point> = hull.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
    let mut best: Option<(f32, Rect)> = None;
    let edges = if points.len() < 2 { 1 } else { points.len() };
    for i in 0..edges {
        let along = if points.len() < 2 {
            [1.0, 0.0]
        } else {
            unit(points[i], points[(i + 1) % points.len()])
        };
        let across = [-along[1], along[0]];
        let (mut lo_a, mut hi_a, mut lo_b, mut hi_b) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &points {
            let (a, b) = (p[0] * along[0] + p[1] * along[1], p[0] * across[0] + p[1] * across[1]);
            (lo_a, hi_a, lo_b, hi_b) = (lo_a.min(a), hi_a.max(a), lo_b.min(b), hi_b.max(b));
        }
        let area = (hi_a - lo_a) * (hi_b - lo_b);
        if best.as_ref().is_none_or(|(least, _)| area < *least) {
            let corner = |a: f32, b: f32| [a * along[0] + b * across[0], a * along[1] + b * across[1]];
            let rect = Rect {
                corners: [
                    corner(lo_a, lo_b),
                    corner(hi_a, lo_b),
                    corner(hi_a, hi_b),
                    corner(lo_a, hi_b),
                ],
                width: hi_a - lo_a,
                height: hi_b - lo_b,
            };
            best = Some((area, rect));
        }
    }
    best.map(|(_, rect)| rect).unwrap_or(Rect {
        corners: [[0.0; 2]; 4],
        width: 0.0,
        height: 0.0,
    })
}

pub fn in_reading_order(corners: [Point; 4]) -> [Point; 4] {
    let mut by_x = corners;
    by_x.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let (mut left, mut right) = ([by_x[0], by_x[1]], [by_x[2], by_x[3]]);
    left.sort_by(|a, b| a[1].total_cmp(&b[1]));
    right.sort_by(|a, b| a[1].total_cmp(&b[1]));
    [left[0], right[0], right[1], left[1]]
}

fn distance(a: Point, b: Point) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn homography(from: [Point; 4], to: [Point; 4]) -> Option<[f64; 8]> {
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let ([x, y], [u, v]) = (from[i].map(f64::from), to[i].map(f64::from));
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let pivot_row = m[col];
        for (row, values) in m.iter_mut().enumerate() {
            if row != col {
                let factor = values[col] / pivot_row[col];
                for (value, &by) in values[col..].iter_mut().zip(&pivot_row[col..]) {
                    *value -= factor * by;
                }
            }
        }
    }
    Some(std::array::from_fn(|i| m[i][8] / m[i][i]))
}

#[derive(Clone, Copy)]
pub struct Picture<'a> {
    pub rgb: &'a [u8],
    pub width: usize,
    pub height: usize,
}

pub struct Owned {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl Owned {
    pub fn view(&self) -> Picture<'_> {
        Picture {
            rgb: &self.rgb,
            width: self.width,
            height: self.height,
        }
    }
}

pub fn cut_out(picture: Picture, corners: [Point; 4]) -> Option<Owned> {
    let width = distance(corners[0], corners[1]).max(distance(corners[2], corners[3])) as usize;
    let height = distance(corners[0], corners[3]).max(distance(corners[1], corners[2])) as usize;
    if width == 0 || height == 0 || picture.width == 0 || picture.height == 0 {
        return None;
    }
    let (w, h) = (width as f32, height as f32);
    let h_map = homography([[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], corners)?;
    let mut rgb = vec![0u8; width * height * 3];
    let weights = |t: f64| {
        const A: f64 = -0.75;
        let near = |d: f64| ((A + 2.0) * d - (A + 3.0)) * d * d + 1.0;
        let far = |d: f64| ((A * d - 5.0 * A) * d + 8.0 * A) * d - 4.0 * A;
        [far(1.0 + t), near(t), near(1.0 - t), far(2.0 - t)]
    };
    let (last_x, last_y) = (picture.width as i64 - 1, picture.height as i64 - 1);
    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f64, y as f64);
            let d = h_map[6] * fx + h_map[7] * fy + 1.0;
            let sx = (h_map[0] * fx + h_map[1] * fy + h_map[2]) / d;
            let sy = (h_map[3] * fx + h_map[4] * fy + h_map[5]) / d;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (wx, wy) = (weights(sx - x0), weights(sy - y0));
            let mut sum = [0f64; 3];
            for (j, wyj) in wy.iter().enumerate() {
                let py = (y0 as i64 - 1 + j as i64).clamp(0, last_y) as usize;
                for (i, wxi) in wx.iter().enumerate() {
                    let px = (x0 as i64 - 1 + i as i64).clamp(0, last_x) as usize;
                    let pixel = &picture.rgb[(py * picture.width + px) * 3..][..3];
                    for c in 0..3 {
                        sum[c] += f64::from(pixel[c]) * wxi * wyj;
                    }
                }
            }
            for c in 0..3 {
                rgb[(y * width + x) * 3 + c] = sum[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let cut = Owned { rgb, width, height };
    Some(if height as f32 >= 1.5 * width as f32 {
        turned_anticlockwise(&cut)
    } else {
        cut
    })
}

fn turned_anticlockwise(picture: &Owned) -> Owned {
    let (w, h) = (picture.width, picture.height);
    let mut rgb = vec![0u8; w * h * 3];
    for y in 0..w {
        for x in 0..h {
            let from = (x * w + (w - 1 - y)) * 3;
            rgb[(y * h + x) * 3..][..3].copy_from_slice(&picture.rgb[from..from + 3]);
        }
    }
    Owned {
        rgb,
        width: h,
        height: w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_area_rect_follows_rotated_bar() {
        let points: Vec<[i32; 2]> = (0..20).flat_map(|i| [[i, i], [i + 1, i], [i, i + 1]]).collect();
        let rect = min_area_rect(&convex_hull(points));
        let (long, short) = (rect.width.max(rect.height), rect.short_side());
        assert!((long - 19.0 * 2f32.sqrt()).abs() < 1.5, "{rect:?}");
        assert!(short < 1.5, "{rect:?}");
    }

    #[test]
    fn crop_rotates_tall_regions() {
        let rgb: Vec<u8> = (0..2)
            .flat_map(|_| (0..6).flat_map(|x| if x < 3 { [255, 0, 0] } else { [0, 0, 255] }))
            .collect();
        let picture = Picture {
            rgb: &rgb,
            width: 6,
            height: 2,
        };
        let cut = cut_out(picture, [[0.0, 0.0], [6.0, 0.0], [6.0, 2.0], [0.0, 2.0]]).unwrap();
        assert_eq!((cut.width, cut.height), (6, 2));
        assert_eq!(&cut.rgb[..3], &[255, 0, 0]);
        assert_eq!(&cut.rgb[15..18], &[0, 0, 255]);
        let flipped = cut_out(picture, [[6.0, 2.0], [0.0, 2.0], [0.0, 0.0], [6.0, 0.0]]).unwrap();
        assert_eq!(&flipped.rgb[..3], &[0, 0, 255]);
        let column: Vec<u8> = (0..6)
            .flat_map(|y| if y < 3 { [255, 0, 0] } else { [0, 0, 255] })
            .collect();
        let tall = cut_out(
            Picture {
                rgb: &column,
                width: 1,
                height: 6,
            },
            [[0.0, 0.0], [1.0, 0.0], [1.0, 6.0], [0.0, 6.0]],
        )
        .unwrap();
        assert_eq!((tall.width, tall.height), (6, 1));
        assert_eq!(&tall.rgb[..3], &[255, 0, 0]);
    }
}
