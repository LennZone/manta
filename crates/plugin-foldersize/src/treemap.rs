//! Squarified treemap layout (Bruls, Huizing, van Wijk 2000).
//!
//! [`squarify`] splits a rectangle into one rectangle per value, with areas
//! proportional to the values and aspect ratios close to 1. The rectangles
//! never overlap and together fill the bounds exactly.

use egui::{pos2, Rect};

/// Lay out `values` (sorted descending for the best result; zeros and
/// negatives get empty rectangles) in `bounds`. Returns one rectangle per
/// value, in the same order.
pub fn squarify(values: &[f64], bounds: Rect) -> Vec<Rect> {
    let n = values.len();
    let mut out = vec![Rect::from_min_size(bounds.min, egui::Vec2::ZERO); n];
    let total: f64 = values.iter().map(|v| v.max(0.0)).sum();
    if total <= 0.0 || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return out;
    }
    // Work in f64 to keep the edges exact.
    let (mut x, mut y) = (bounds.min.x as f64, bounds.min.y as f64);
    let (mut w, mut h) = (bounds.width() as f64, bounds.height() as f64);
    let scale = w * h / total;
    let idx: Vec<usize> = (0..n).filter(|&i| values[i] > 0.0).collect();
    let areas: Vec<f64> = idx.iter().map(|&i| values[i] * scale).collect();

    let mut i = 0;
    while i < areas.len() {
        let short = w.min(h);
        let mut j = i + 1;
        let mut sum = areas[i];
        let mut best = worst(&areas[i..j], sum, short);
        while j < areas.len() {
            let s2 = sum + areas[j];
            let wr = worst(&areas[i..=j], s2, short);
            if wr > best {
                break;
            }
            best = wr;
            sum = s2;
            j += 1;
        }
        let last_row = j == areas.len();
        if w >= h {
            // A column on the left, items stacked top to bottom.
            let cw = if last_row { w } else { sum / h };
            let mut yy = y;
            for k in i..j {
                let ih = if k + 1 == j {
                    y + h - yy
                } else {
                    areas[k] / cw
                };
                out[idx[k]] = rect(x, yy, cw, ih);
                yy += ih;
            }
            x += cw;
            w -= cw;
        } else {
            // A row on top, items left to right.
            let rh = if last_row { h } else { sum / w };
            let mut xx = x;
            for k in i..j {
                let iw = if k + 1 == j {
                    x + w - xx
                } else {
                    areas[k] / rh
                };
                out[idx[k]] = rect(xx, y, iw, rh);
                xx += iw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::from_min_max(
        pos2(x as f32, y as f32),
        pos2((x + w) as f32, (y + h) as f32),
    )
}

/// The worst aspect ratio of a row with these areas laid along `side`.
fn worst(row: &[f64], sum: f64, side: f64) -> f64 {
    let (mut min, mut max) = (f64::INFINITY, 0.0f64);
    for &a in row {
        min = min.min(a);
        max = max.max(a);
    }
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn check(values: &[f64], bounds: Rect) -> Vec<Rect> {
        let rects = squarify(values, bounds);
        assert_eq!(rects.len(), values.len());
        let total: f64 = values.iter().sum();
        let area = (bounds.width() * bounds.height()) as f64;
        let mut covered = 0.0;
        for (v, r) in values.iter().zip(&rects) {
            let a = (r.width() * r.height()) as f64;
            covered += a;
            // Proportional (within float error).
            let want = v / total * area;
            assert!(
                (a - want).abs() <= want * 1e-3 + 0.05,
                "area {a} vs {want} for {v}"
            );
            // Inside the bounds.
            assert!(
                bounds.expand(1e-3).contains_rect(*r),
                "{r:?} outside {bounds:?}"
            );
            assert!(r.width() >= 0.0 && r.height() >= 0.0);
        }
        // Fills the bounds.
        assert!(
            (covered - area).abs() < area * 1e-4,
            "covered {covered} of {area}"
        );
        // No overlaps.
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                let i = rects[a].intersect(rects[b]);
                let overlap = if i.is_positive() {
                    i.width() * i.height()
                } else {
                    0.0
                };
                assert!(overlap < 1e-2, "{:?} overlaps {:?}", rects[a], rects[b]);
            }
        }
        rects
    }

    #[test]
    fn classic_example() {
        // The example from the paper: 6 6 4 3 2 2 1 in a 6×4 rectangle.
        let b = Rect::from_min_size(pos2(0.0, 0.0), vec2(6.0, 4.0));
        let r = check(&[6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0], b);
        // The first two form a column of width 3 on the left.
        assert!((r[0].width() - 3.0).abs() < 1e-4 && (r[1].width() - 3.0).abs() < 1e-4);
        // Aspect ratios stay reasonable.
        for x in &r {
            let ar = (x.width() / x.height()).max(x.height() / x.width());
            assert!(ar < 3.0, "{x:?}");
        }
    }

    #[test]
    fn many_shapes() {
        let bounds = [
            Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 200.0)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 480.0)),
            Rect::from_min_size(pos2(-5.0, 3.5), vec2(1.0, 1.0)),
        ];
        let sets: Vec<Vec<f64>> = vec![
            vec![1.0],
            vec![5.0, 5.0],
            vec![100.0, 1.0, 1.0, 1.0],
            (1..60).rev().map(|i| (i * i) as f64).collect(),
            vec![1e12, 3e9, 2.5e9, 7e6, 1.0],
            (0..200).map(|i| 1.0 + (i % 7) as f64).collect(),
        ];
        for b in bounds {
            for s in &sets {
                check(s, b);
            }
        }
    }

    #[test]
    fn zeros_and_empty() {
        let b = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0));
        assert!(squarify(&[], b).is_empty());
        let r = squarify(&[0.0, 0.0], b);
        assert!(r.iter().all(|r| r.area() == 0.0));
        let r = squarify(&[10.0, 0.0, 5.0], b);
        assert_eq!(r[1].area(), 0.0);
        assert!((r[0].area() + r[2].area() - 5000.0).abs() < 0.1);
    }
}
