//! Windows of whole UTC days, the way the analytics pages ask for them.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use media_core::text::from_secs;
use media_core::{Insights, Point, Series};

const DAY: i64 = 86_400;

/// A window of whole UTC days, `[from, to)` in Unix seconds.
#[derive(Debug, Clone, Copy)]
pub struct Window {
  pub from: i64,
  pub to: i64,
}

impl Window {
  /// The last `days` days, today included: it ends at the next UTC midnight, as in the web client.
  pub fn last(days: u32) -> Self {
    let to = (now().div_euclid(DAY) + 1) * DAY;
    Self {
      from: to - i64::from(days.max(1)) * DAY,
      to,
    }
  }

  /// From the day of `start` (Unix seconds) through today.
  pub fn since(start: i64) -> Self {
    Self {
      from: start.div_euclid(DAY) * DAY,
      to: Self::last(1).to,
    }
  }

  /// Without the days before `start` (Unix seconds).
  pub fn clip(self, start: i64) -> Self {
    Self {
      from: self.from.max(start.div_euclid(DAY) * DAY),
      ..self
    }
  }

  pub fn contains(self, secs: i64) -> bool {
    (self.from..self.to).contains(&secs)
  }

  /// Start as RFC 3339 (the `from_time` of the analytics queries).
  pub fn start_iso(self) -> String {
    stamp(self.from)
  }

  /// End (exclusive) as RFC 3339.
  pub fn end_iso(self) -> String {
    stamp(self.to)
  }

  /// Bounds in milliseconds (the audience queries' `*_time_incl` / `*_time_excl`).
  pub fn millis(self) -> (i64, i64) {
    (self.from * 1000, self.to * 1000)
  }

  /// A daily series over the whole window from `values` by `YYYY-MM-DD` (missing days as 0).
  pub fn daily(self, metric: &str, values: &BTreeMap<String, u64>) -> Series {
    let points = (self.from..self.to)
      .step_by(DAY as usize)
      .map(|t| {
        let date = day(t);
        let value = values.get(&date).copied().unwrap_or(0).into();
        Point { date, value }
      })
      .collect();
    Series {
      metric: metric.into(),
      points,
    }
  }

  /// First and last day covered, `YYYY-MM-DD`.
  pub fn days(self) -> (String, String) {
    (day(self.from), day(self.to - DAY))
  }

  /// Set `from` / `to` of `insights` to the days covered.
  pub fn describe(self, insights: &mut Insights) {
    let (from, to) = self.days();
    insights.from = Some(from);
    insights.to = Some(to);
  }
}

/// Unix seconds now.
pub fn now() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| d.as_secs() as i64)
}

/// RFC 3339 of Unix seconds.
fn stamp(secs: i64) -> String {
  from_secs(secs).map(|t| t.to_string()).unwrap_or_default()
}

fn day(secs: i64) -> String {
  from_secs(secs)
    .map(|t| t.strftime("%Y-%m-%d").to_string())
    .unwrap_or_default()
}
