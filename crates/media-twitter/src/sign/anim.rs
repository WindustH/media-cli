//! The CSS-like animation the transaction key is sampled from: a colour fade
//! and a rotation eased by a cubic bezier, rendered as a hex string. Float
//! operations keep the reference's order so results match bit for bit.

/// Animation key for one frame row at `time` in `[0, 1)`.
pub fn animate(row: &[u32], time: f64) -> Option<String> {
  if row.len() < 11 {
    return None;
  }
  let v = |i: usize| f64::from(row[i]);
  let from_color = [v(0), v(1), v(2), 1.0];
  let to_color = [v(3), v(4), v(5), 1.0];
  let to_rotation = solve(v(6), 60.0, 360.0, true);
  let curves: Vec<f64> = row[7..]
    .iter()
    .enumerate()
    .map(|(i, x)| {
      solve(
        f64::from(*x),
        if i % 2 == 1 { -1.0 } else { 0.0 },
        1.0,
        false,
      )
    })
    .collect();
  let t = cubic(&curves, time);

  let mut out = String::new();
  for (from, to) in from_color.iter().zip(to_color).take(3) {
    let c = interpolate(*from, to, t).clamp(0.0, 255.0);
    out.push_str(&format!("{:x}", c.round_ties_even() as i64));
  }
  let rad = interpolate(0.0, to_rotation, t).to_radians();
  for value in [rad.cos(), -rad.sin(), rad.sin(), rad.cos()] {
    let mut rounded = round2(value);
    if rounded < 0.0 {
      rounded = -rounded;
    }
    let hex = float_to_hex(rounded);
    if hex.starts_with('.') {
      out.push('0');
      out.push_str(&hex.to_lowercase());
    } else if hex.is_empty() {
      out.push('0');
    } else {
      out.push_str(&hex);
    }
  }
  out.push_str("00");
  Some(out.replace(['.', '-'], ""))
}

fn solve(value: f64, min: f64, max: f64, floor: bool) -> f64 {
  let result = value * (max - min) / 255.0 + min;
  if floor {
    result.floor()
  } else {
    round2(result)
  }
}

fn interpolate(from: f64, to: f64, f: f64) -> f64 {
  from * (1.0 - f) + to * f
}

/// Python's `round(x, 2)`: correctly rounded, ties to even (as Rust's formatter).
fn round2(x: f64) -> f64 {
  format!("{x:.2}").parse().unwrap_or(x)
}

fn bezier(a: f64, b: f64, m: f64) -> f64 {
  3.0 * a * (1.0 - m) * (1.0 - m) * m + 3.0 * b * (1.0 - m) * m * m + m * m * m
}

/// Cubic bezier easing `curves = [x1, y1, x2, y2]` evaluated at `time`.
fn cubic(c: &[f64], time: f64) -> f64 {
  if time <= 0.0 {
    let gradient = if c[0] > 0.0 {
      c[1] / c[0]
    } else if c[1] == 0.0 && c[2] > 0.0 {
      c[3] / c[2]
    } else {
      0.0
    };
    return gradient * time;
  }
  if time >= 1.0 {
    let gradient = if c[2] < 1.0 {
      (c[3] - 1.0) / (c[2] - 1.0)
    } else if c[2] == 1.0 && c[0] < 1.0 {
      (c[1] - 1.0) / (c[0] - 1.0)
    } else {
      0.0
    };
    return 1.0 + gradient * (time - 1.0);
  }
  let (mut start, mut end, mut mid) = (0.0, 1.0, 0.0);
  // Bisection; the reference loops until convergence, which happens long before the cap.
  for _ in 0..2000 {
    if start >= end {
      break;
    }
    mid = (start + end) / 2.0;
    let x = bezier(c[0], c[2], mid);
    if (time - x).abs() < 0.00001 {
      return bezier(c[1], c[3], mid);
    }
    if x < time {
      start = mid;
    } else {
      end = mid;
    }
  }
  bezier(c[1], c[3], mid)
}

/// The reference's float to hex conversion (upper case digits, `.` before the fraction).
fn float_to_hex(mut x: f64) -> String {
  let digit = |d: u32| char::from_digit(d, 16).unwrap_or('0').to_ascii_uppercase();
  let mut quotient = x.trunc();
  let mut fraction = x - quotient;
  let mut int_part = Vec::new();
  while quotient > 0.0 {
    quotient = (x / 16.0).trunc();
    let remainder = (x - quotient * 16.0).trunc() as u32;
    int_part.insert(0, digit(remainder));
    x = quotient;
  }
  let mut out: String = int_part.into_iter().collect();
  if fraction == 0.0 {
    return out;
  }
  out.push('.');
  while fraction > 0.0 {
    fraction *= 16.0;
    let integer = fraction.trunc();
    fraction -= integer;
    out.push(digit(integer as u32));
  }
  out
}
