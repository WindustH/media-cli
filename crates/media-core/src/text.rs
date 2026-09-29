//! Small text helpers: counts, durations, truncation, HTML to text, timestamps.

use jiff::{Timestamp, tz::TimeZone};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Parse counters such as `1234`, `1,234`, `1.2万`, `3亿`, `4.5k`, `2w`, `10+`.
pub fn parse_count(s: &str) -> Option<u64> {
  let s = s.trim().trim_end_matches('+').replace(',', "");
  if s.is_empty() {
    return None;
  }
  let (num, mul) = match s.chars().last()? {
    '万' | 'w' | 'W' => (&s[..s.len() - s.chars().last()?.len_utf8()], 10_000.0),
    '亿' => (&s[..s.len() - '亿'.len_utf8()], 100_000_000.0),
    'k' | 'K' => (&s[..s.len() - 1], 1_000.0),
    'm' | 'M' => (&s[..s.len() - 1], 1_000_000.0),
    _ => (s.as_str(), 1.0),
  };
  num
    .trim()
    .parse::<f64>()
    .ok()
    .map(|n| (n * mul).round().max(0.0) as u64)
}

/// `1234` -> `1.2k`, `123456` -> `12.3万` style is avoided on purpose: one scale for all platforms.
pub fn fmt_count(n: u64) -> String {
  match n {
    0..1_000 => n.to_string(),
    1_000..1_000_000 => trim_dot(format!("{:.1}", n as f64 / 1e3)) + "k",
    1_000_000..1_000_000_000 => trim_dot(format!("{:.1}", n as f64 / 1e6)) + "M",
    _ => trim_dot(format!("{:.1}", n as f64 / 1e9)) + "B",
  }
}

fn trim_dot(s: String) -> String {
  s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// `3725` seconds -> `1:02:05`.
pub fn fmt_duration(secs: f64) -> String {
  let t = secs.max(0.0).round() as u64;
  let (h, m, s) = (t / 3600, t / 60 % 60, t % 60);
  if h > 0 {
    format!("{h}:{m:02}:{s:02}")
  } else {
    format!("{m}:{s:02}")
  }
}

/// Collapse whitespace (including newlines) into single spaces.
pub fn one_line(s: &str) -> String {
  s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cut to a display width, appending `…` when shortened. Wide (CJK) characters count as 2.
pub fn truncate(s: &str, width: usize) -> String {
  if s.width() <= width {
    return s.to_owned();
  }
  let mut out = String::new();
  let mut w = 0;
  for c in s.chars() {
    let cw = c.width().unwrap_or(0);
    if w + cw + 1 > width {
      break;
    }
    out.push(c);
    w += cw;
  }
  out.push('…');
  out
}

/// Render HTML (answers, articles, descriptions) as plain text.
pub fn html_to_text(html: &str) -> String {
  if !html.contains('<') && !html.contains('&') {
    return html.trim().to_owned();
  }
  let text = html2text::config::plain_no_decorate()
    .string_from_read(html.as_bytes(), 10_000)
    .unwrap_or_else(|_| html.to_owned());
  let mut out = String::with_capacity(text.len());
  let mut blank = 0;
  for line in text.lines().map(str::trim_end) {
    if line.is_empty() {
      blank += 1;
      if blank > 1 {
        continue;
      }
    } else {
      blank = 0;
    }
    out.push_str(line);
    out.push('\n');
  }
  out.trim().to_owned()
}

/// Unix seconds -> timestamp (0 and negatives mean "unknown").
pub fn from_secs(secs: i64) -> Option<Timestamp> {
  (secs > 0)
    .then(|| Timestamp::from_second(secs).ok())
    .flatten()
}

/// Unix milliseconds -> timestamp.
pub fn from_millis(ms: i64) -> Option<Timestamp> {
  (ms > 0)
    .then(|| Timestamp::from_millisecond(ms).ok())
    .flatten()
}

/// Seconds or milliseconds, guessed by magnitude.
pub fn from_unix(v: i64) -> Option<Timestamp> {
  if v > 100_000_000_000 {
    from_millis(v)
  } else {
    from_secs(v)
  }
}

/// Parse `Wed Oct 10 20:19:24 +0000 2018` (Twitter) or RFC 3339.
pub fn parse_time(s: &str) -> Option<Timestamp> {
  s.parse::<Timestamp>().ok().or_else(|| {
    jiff::fmt::strtime::parse("%a %b %d %H:%M:%S %z %Y", s)
      .ok()?
      .to_timestamp()
      .ok()
  })
}

/// Local, compact rendering for tables: `09-28 14:03`, or `2024-09-28` for older years.
pub fn fmt_time(ts: Timestamp) -> String {
  let local = ts.to_zoned(TimeZone::system());
  let now = Timestamp::now().to_zoned(TimeZone::system());
  if local.year() == now.year() {
    local.strftime("%m-%d %H:%M").to_string()
  } else {
    local.strftime("%Y-%m-%d").to_string()
  }
}

/// Filesystem-safe file name stem.
pub fn file_stem(s: &str) -> String {
  let cleaned: String = s
    .chars()
    .map(|c| {
      if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
        '_'
      } else {
        c
      }
    })
    .collect();
  truncate(cleaned.trim().trim_start_matches('.'), 80)
    .trim_end_matches('…')
    .trim()
    .to_owned()
}
