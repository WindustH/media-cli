//! Terminal rendering of [`Data`] for people.

use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Table, presets};
use owo_colors::{OwoColorize, Stream};

use crate::error::Error;
use crate::model::{
  Action, AuthStatus, Collection, Comment, Data, Downloaded, Insights, Metrics, Notification, Page,
  Post, Transcript, User,
};
use crate::text::{fmt_count, fmt_duration, fmt_time, one_line, truncate};

pub fn render(data: &Data) {
  match data {
    Data::Posts(page) => list(page, post_table),
    Data::Users(page) => list(page, user_table),
    Data::Comments(page) => list(page, comment_tree),
    Data::Collections(page) => list(page, collection_table),
    Data::Notifications(page) => list(page, notification_table),
    Data::Post(post) => post_card(post),
    Data::User(user) => user_card(user),
    Data::Action(action) => action_line(action),
    Data::Auth(auth) => auth_line(auth),
    Data::Counts(counts) => {
      for (k, v) in counts {
        println!(
          "{:<16} {}",
          k.if_supports_color(Stream::Stdout, |t| t.dimmed()),
          v
        );
      }
    }
    Data::Transcript(t) => transcript(t),
    Data::Downloads(files) => downloads(files),
    Data::Insights(i) => insights(i),
    Data::Value(v) => print!("{}", serde_saphyr::to_string(v).unwrap_or_default()),
  }
}

pub fn error(e: &Error) {
  eprintln!(
    "{} {}",
    "error:".if_supports_color(Stream::Stderr, |t| t.red()),
    e.message
  );
  if let Some(hint) = &e.hint {
    eprintln!(
      "{} {}",
      "hint:".if_supports_color(Stream::Stderr, |t| t.yellow()),
      hint
    );
  }
}

pub fn note(message: &str) {
  eprintln!(
    "{}",
    message.if_supports_color(Stream::Stderr, |t| t.dimmed())
  );
}

fn list<T>(page: &Page<T>, draw: fn(&[T])) {
  if page.items.is_empty() {
    note("(no results)");
  } else {
    draw(&page.items);
  }
  if let Some(cursor) = &page.next_cursor {
    note(&format!("more: --cursor {cursor}"));
  }
}

fn table(header: &[&str]) -> Table {
  let mut t = Table::new();
  t.load_style(presets::NOTHING)
    .set_content_arrangement(ContentArrangement::Dynamic);
  t.set_header(
    header
      .iter()
      .map(|h| Cell::new(h).add_attribute(Attribute::Bold)),
  );
  t
}

fn num(n: Option<u64>) -> Cell {
  Cell::new(n.map(fmt_count).unwrap_or_default()).set_alignment(CellAlignment::Right)
}

fn index(i: usize) -> Cell {
  Cell::new(i + 1).add_attribute(Attribute::Dim)
}

fn user_label(u: &User) -> String {
  match &u.handle {
    Some(h) if h != &u.name && !h.is_empty() => format!("{} @{}", u.name, h),
    _ => u.name.clone(),
  }
}

fn headline(p: &Post) -> String {
  let title = p.title.as_deref().map(one_line).unwrap_or_default();
  let text = p.text.as_deref().map(one_line).unwrap_or_default();
  let s = if title.is_empty() { text } else { title };
  truncate(&s, 80)
}

fn post_table(posts: &[Post]) {
  let views = posts.iter().any(|p| p.metrics.views.is_some());
  let mut header = vec!["#", "Content", "Author", "Likes", "Cmts"];
  if views {
    header.push("Views");
  }
  header.extend(["Time", "ID"]);
  let mut t = table(&header);
  for (i, p) in posts.iter().enumerate() {
    let mut row = vec![
      index(i),
      Cell::new(headline(p)),
      Cell::new(
        p.author
          .as_ref()
          .map(|a| truncate(&a.name, 16))
          .unwrap_or_default(),
      ),
      num(p.metrics.likes),
      num(p.metrics.comments),
    ];
    if views {
      row.push(num(p.metrics.views));
    }
    row.push(Cell::new(p.created_at.map(fmt_time).unwrap_or_default()));
    row.push(Cell::new(&p.id).add_attribute(Attribute::Dim));
    t.add_row(row);
  }
  println!("{t}");
}

fn user_table(users: &[User]) {
  let mut t = table(&["#", "Name", "Followers", "Posts", "Bio", "ID"]);
  for (i, u) in users.iter().enumerate() {
    t.add_row(vec![
      index(i),
      Cell::new(truncate(&user_label(u), 32)),
      num(u.stats.followers),
      num(u.stats.posts),
      Cell::new(truncate(&one_line(u.bio.as_deref().unwrap_or("")), 48)),
      Cell::new(&u.id).add_attribute(Attribute::Dim),
    ]);
  }
  println!("{t}");
}

fn collection_table(items: &[Collection]) {
  let mut t = table(&[
    "#",
    "Name",
    "Kind",
    "Items",
    "Followers",
    "Description",
    "ID",
  ]);
  for (i, c) in items.iter().enumerate() {
    t.add_row(vec![
      index(i),
      Cell::new(truncate(&c.name, 32)),
      Cell::new(&c.kind),
      num(c.items),
      num(c.followers),
      Cell::new(truncate(
        &one_line(c.description.as_deref().unwrap_or("")),
        40,
      )),
      Cell::new(&c.id).add_attribute(Attribute::Dim),
    ]);
  }
  println!("{t}");
}

fn notification_table(items: &[Notification]) {
  let mut t = table(&["#", "Kind", "From", "Text", "Time"]);
  for (i, n) in items.iter().enumerate() {
    let mut text = one_line(&n.text);
    if let Some(target) = &n.target {
      text = format!("{text} · {}", one_line(target));
    }
    let kind = if n.unread == Some(true) {
      format!("{} •", n.kind)
    } else {
      n.kind.clone()
    };
    t.add_row(vec![
      index(i),
      Cell::new(kind),
      Cell::new(
        n.actor
          .as_ref()
          .map(|a| truncate(&a.name, 16))
          .unwrap_or_default(),
      ),
      Cell::new(truncate(&text, 72)),
      Cell::new(n.created_at.map(fmt_time).unwrap_or_default()),
    ]);
  }
  println!("{t}");
}

fn comment_tree(comments: &[Comment]) {
  for (i, c) in comments.iter().enumerate() {
    comment(c, 0, Some(i + 1));
  }
}

fn comment(c: &Comment, depth: usize, index: Option<usize>) {
  let pad = "  ".repeat(depth);
  let mut meta = Vec::new();
  if let Some(t) = c.created_at {
    meta.push(fmt_time(t));
  }
  if let Some(l) = c.likes.filter(|l| *l > 0) {
    meta.push(format!("{} likes", fmt_count(l)));
  }
  if let Some(r) = c.reply_count.filter(|r| *r > 0) {
    meta.push(format!("{} replies", fmt_count(r)));
  }
  if let Some(loc) = &c.location {
    meta.push(loc.clone());
  }
  meta.push(format!("id {}", c.id));
  let name = c.author.as_ref().map(|a| a.name.as_str()).unwrap_or("?");
  let name = match &c.reply_to {
    Some(to) => format!("{name} → {to}"),
    None => name.to_owned(),
  };
  let marker = match index {
    Some(i) => format!("{i}."),
    None => "↳".to_owned(),
  };
  println!(
    "{pad}{} {} {}",
    marker.if_supports_color(Stream::Stdout, |t| t.dimmed()),
    name.if_supports_color(Stream::Stdout, |t| t.bold()),
    format!("· {}", meta.join(" · ")).if_supports_color(Stream::Stdout, |t| t.dimmed())
  );
  for line in c.text.lines() {
    println!("{pad}   {line}");
  }
  for r in &c.replies {
    comment(r, depth + 1, None);
  }
}

fn metrics_line(m: &Metrics) -> String {
  let mut parts = Vec::new();
  for (label, v) in [
    ("views", m.views),
    ("likes", m.likes),
    ("comments", m.comments),
    ("shares", m.shares),
    ("favorites", m.favorites),
  ] {
    if let Some(v) = v {
      parts.push(format!("{} {label}", fmt_count(v)));
    }
  }
  for (k, v) in &m.other {
    parts.push(format!("{} {k}", fmt_count(*v)));
  }
  parts.join(" · ")
}

fn post_card(p: &Post) {
  if let Some(title) = &p.title {
    println!("{}", title.if_supports_color(Stream::Stdout, |t| t.bold()));
  }
  let mut meta = vec![p.kind.clone()];
  if let Some(a) = &p.author {
    meta.push(user_label(a));
  }
  if let Some(t) = p.created_at {
    meta.push(fmt_time(t));
  }
  meta.push(format!("id {}", p.id));
  println!(
    "{}",
    meta
      .join(" · ")
      .if_supports_color(Stream::Stdout, |t| t.dimmed())
  );
  let metrics = metrics_line(&p.metrics);
  if !metrics.is_empty() {
    println!("{metrics}");
  }
  if let Some(url) = &p.url {
    println!("{}", url.if_supports_color(Stream::Stdout, |t| t.cyan()));
  }
  if let Some(text) = p.text.as_deref().filter(|t| !t.is_empty()) {
    println!("\n{text}");
  }
  if !p.tags.is_empty() {
    println!(
      "\n{}",
      p.tags
        .iter()
        .map(|t| format!("#{t}"))
        .collect::<Vec<_>>()
        .join(" ")
    );
  }
  if !p.media.is_empty() {
    println!();
    for m in &p.media {
      let dur = m
        .duration
        .map(|d| format!(" ({})", fmt_duration(d)))
        .unwrap_or_default();
      println!(
        "{}{dur} {}",
        format!("{:?}", m.kind).to_lowercase(),
        m.url.if_supports_color(Stream::Stdout, |t| t.dimmed())
      );
    }
  }
  if let Some(q) = &p.quoted {
    println!(
      "\n{}",
      "┃ quoted".if_supports_color(Stream::Stdout, |t| t.dimmed())
    );
    let who = q.author.as_ref().map(user_label).unwrap_or_default();
    println!("┃ {who}");
    for line in headline(q).lines() {
      println!("┃ {line}");
    }
  }
}

fn user_card(u: &User) {
  println!(
    "{}",
    user_label(u).if_supports_color(Stream::Stdout, |t| t.bold())
  );
  let mut meta = vec![format!("id {}", u.id)];
  if u.verified {
    meta.push("verified".into());
  }
  if let Some(l) = &u.location {
    meta.push(l.clone());
  }
  if let Some(true) = u.followed {
    meta.push("following".into());
  }
  println!(
    "{}",
    meta
      .join(" · ")
      .if_supports_color(Stream::Stdout, |t| t.dimmed())
  );
  let s = &u.stats;
  let mut stats = Vec::new();
  for (label, v) in [
    ("followers", s.followers),
    ("following", s.following),
    ("posts", s.posts),
    ("likes", s.likes),
  ] {
    if let Some(v) = v {
      stats.push(format!("{} {label}", fmt_count(v)));
    }
  }
  for (k, v) in &s.other {
    stats.push(format!("{} {k}", fmt_count(*v)));
  }
  if !stats.is_empty() {
    println!("{}", stats.join(" · "));
  }
  if let Some(url) = &u.url {
    println!("{}", url.if_supports_color(Stream::Stdout, |t| t.cyan()));
  }
  if let Some(bio) = u.bio.as_deref().filter(|b| !b.is_empty()) {
    println!("\n{bio}");
  }
}

fn action_line(a: &Action) {
  let mark = if a.ok {
    "✓"
      .if_supports_color(Stream::Stdout, |t| t.green())
      .to_string()
  } else {
    "✗".to_owned()
  };
  let mut line = format!("{mark} {} {}", a.action, a.target);
  if let Some(id) = &a.id {
    line.push_str(&format!(" → {id}"));
  }
  println!("{line}");
  if let Some(url) = &a.url {
    println!("{}", url.if_supports_color(Stream::Stdout, |t| t.cyan()));
  }
  if let Some(m) = &a.message {
    note(m);
  }
}

fn auth_line(a: &AuthStatus) {
  match (&a.user, a.authenticated) {
    (Some(u), true) => {
      let source = a
        .source
        .as_deref()
        .map(|s| format!(" (via {s})"))
        .unwrap_or_default();
      println!(
        "{} logged in as {} · id {}{source}",
        "✓".if_supports_color(Stream::Stdout, |t| t.green()),
        user_label(u),
        u.id
      );
    }
    _ => println!(
      "{} not logged in",
      "✗".if_supports_color(Stream::Stdout, |t| t.red())
    ),
  }
  if let Some(m) = &a.message {
    note(m);
  }
}

fn transcript(t: &Transcript) {
  note(&format!("language: {}", t.lang));
  for c in &t.cues {
    println!(
      "{} {}",
      format!("[{}]", fmt_duration(c.from)).if_supports_color(Stream::Stdout, |t| t.dimmed()),
      c.text
    );
  }
}

fn downloads(files: &[Downloaded]) {
  for f in files {
    println!(
      "{} {} ({})",
      format!("{:?}", f.kind).to_lowercase(),
      f.path,
      fmt_count(f.bytes) + "B"
    );
  }
}

/// Integers as counts, `*_rate` / `*ratio` / `ctr` as percentages, other decimals rounded.
fn metric(name: &str, v: &serde_json::Value) -> String {
  let Some(n) = v.as_f64() else {
    return crate::output::rows::cell(v);
  };
  if name.ends_with("rate") || name.ends_with("ratio") || name == "ctr" || name.ends_with("_ctr") {
    return format!("{:.1}%", n * 100.0);
  }
  match v.as_u64() {
    Some(u) => fmt_count(u),
    None => format!("{n:.2}")
      .trim_end_matches('0')
      .trim_end_matches('.')
      .to_owned(),
  }
}

fn insights(i: &Insights) {
  let title = i
    .title
    .clone()
    .unwrap_or_else(|| format!("{} insights", i.kind));
  println!("{}", title.if_supports_color(Stream::Stdout, |t| t.bold()));
  let mut meta = vec![i.kind.clone(), i.subject.clone()];
  if let (Some(from), Some(to)) = (&i.from, &i.to) {
    meta.push(format!("{from} → {to}"));
  }
  println!(
    "{}",
    meta
      .join(" · ")
      .if_supports_color(Stream::Stdout, |t| t.dimmed())
  );
  if let Some(url) = &i.url {
    println!("{}", url.if_supports_color(Stream::Stdout, |t| t.cyan()));
  }
  for w in &i.warnings {
    println!(
      "{} {w}",
      "!".if_supports_color(Stream::Stdout, |t| t.yellow())
    );
  }
  if !i.totals.is_empty() {
    let period = i.totals_period.as_deref().map(|p| format!("Value ({p})"));
    let mut t = table(&["Metric", period.as_deref().unwrap_or("Value")]);
    for (k, v) in &i.totals {
      t.add_row(vec![
        Cell::new(k),
        Cell::new(metric(k, v)).set_alignment(CellAlignment::Right),
      ]);
    }
    println!("\n{t}");
  }
  if !i.series.is_empty() {
    // Dates down, metrics across; the most recent two weeks.
    let mut dates: Vec<&str> = i
      .series
      .iter()
      .flat_map(|s| s.points.iter().map(|p| p.date.as_str()))
      .collect();
    dates.sort_unstable();
    dates.dedup();
    let recent = &dates[dates.len().saturating_sub(14)..];
    let mut header = vec!["Date"];
    header.extend(i.series.iter().map(|s| s.metric.as_str()));
    let mut t = table(&header);
    for d in recent {
      let mut row = vec![Cell::new(d)];
      for s in &i.series {
        let v = s
          .points
          .iter()
          .find(|p| p.date == *d)
          .map(|p| metric(&s.metric, &p.value));
        row.push(Cell::new(v.unwrap_or_default()).set_alignment(CellAlignment::Right));
      }
      t.add_row(row);
    }
    println!("\n{t}");
    if dates.len() > recent.len() {
      note(&format!(
        "(last {} of {} days; --format csv for all)",
        recent.len(),
        dates.len()
      ));
    }
  }
  for b in &i.breakdowns {
    let title = match &b.period {
      Some(p) => format!("{} ({p})", b.dimension),
      None => b.dimension.clone(),
    };
    let mut t = table(&[title.as_str(), "Value", "Share", ""]);
    for x in &b.items {
      let ratio = x
        .ratio
        .map(|r| format!("{:.1}%", r * 100.0))
        .unwrap_or_default();
      let bar = "█".repeat((x.ratio.unwrap_or(0.0) * 20.0).round() as usize);
      t.add_row(vec![
        Cell::new(&x.label),
        Cell::new(metric("", &x.value)).set_alignment(CellAlignment::Right),
        Cell::new(ratio).set_alignment(CellAlignment::Right),
        Cell::new(bar).add_attribute(Attribute::Dim),
      ]);
    }
    println!("\n{t}");
  }
}
