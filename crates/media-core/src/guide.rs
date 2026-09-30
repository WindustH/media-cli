//! `media guide`: usage documentation built into the binary.
//!
//! General topics are Markdown files under `crates/media-core/guide/`. A
//! platform's topic is generated from what the binary knows (its commands,
//! option values, login methods) followed by the platform crate's own notes
//! (`PlatformInfo::guide`), so it cannot drift from the real command set.
//! Terminals get light styling; pipes get the Markdown as is.

use std::io::IsTerminal;

use clap::Subcommand;
use owo_colors::{OwoColorize, Stream};

use crate::cli::CommonCommand;
use crate::platform::{Cap, PlatformInfo};

pub struct Topic {
  pub name: &'static str,
  pub summary: &'static str,
  pub body: &'static str,
}

pub const TOPICS: &[Topic] = &[
  Topic {
    name: "start",
    summary: "What media-cli does, first commands, where to get help",
    body: include_str!("../guide/start.md"),
  },
  Topic {
    name: "login",
    summary: "QR codes, browser sessions, cookie strings, environment credentials",
    body: include_str!("../guide/login.md"),
  },
  Topic {
    name: "refs",
    summary: "Ids, links, #N, --limit, --cursor, --since / --until",
    body: include_str!("../guide/refs.md"),
  },
  Topic {
    name: "output",
    summary: "table, json, yaml, jsonl, csv; the envelope, error codes and data shapes",
    body: include_str!("../guide/output.md"),
  },
  Topic {
    name: "analysis",
    summary: "Creator insights, likers, reposts, full comment threads, exports, snapshots",
    body: include_str!("../guide/analysis.md"),
  },
  Topic {
    name: "download",
    summary: "Images, videos, audio only, speech-recognition segments",
    body: include_str!("../guide/download.md"),
  },
  Topic {
    name: "interact",
    summary: "Likes, favorites, comments, follows, posting and deleting",
    body: include_str!("../guide/interact.md"),
  },
  Topic {
    name: "config",
    summary: "Global options, environment variables, files, exit codes",
    body: include_str!("../guide/config.md"),
  },
];

/// Options of shared commands whose values each platform chooses.
const CHOICES: &[(&str, &str, &str)] = &[
  ("search", "sort", "--sort"),
  ("search", "filter", "--filter"),
  ("hot", "category", "--category"),
  ("feed", "kind", "--type"),
  ("comments", "sort", "--sort"),
  ("notifications", "kind", "--type"),
];

fn about(cmd: &clap::Command) -> String {
  cmd.get_about().map(|a| a.to_string()).unwrap_or_default()
}

/// A platform's topic: generated command reference plus its own notes.
pub fn platform(info: &PlatformInfo, cmd: &clap::Command) -> String {
  let mut out = format!("# {} (`{}`)\n\n", info.name, info.id);
  let aliases: Vec<String> = info.aliases.iter().map(|a| format!("`{a}`")).collect();
  if !aliases.is_empty() {
    out.push_str(&format!("Also answers to {}. ", aliases.join(", ")));
  }
  out.push_str(&format!("Home: {}\n\n", info.home));

  let mut visible: Vec<&clap::Command> = cmd
    .get_subcommands()
    .filter(|c| !c.is_hide_set() && c.get_name() != "help")
    .collect();
  // `--help` order (adjusting a subcommand moves it to the end of the list).
  visible.sort_by_key(|c| c.get_display_order());
  let (shared, own): (Vec<&clap::Command>, Vec<&clap::Command>) = visible
    .into_iter()
    .partition(|c| CommonCommand::has_subcommand(c.get_name()));
  out.push_str("## Commands\n\n");
  for c in &shared {
    out.push_str(&format!("- `{}`: {}\n", c.get_name(), about(c)));
  }
  if !own.is_empty() {
    out.push_str(&format!("\n## Only on {}\n\n", info.name));
    for c in &own {
      out.push_str(&format!("- `{}`: {}\n", c.get_name(), about(c)));
    }
  }

  let mut values = String::new();
  for (sub, arg, flag) in CHOICES {
    let Some(c) = cmd.find_subcommand(sub).filter(|c| !c.is_hide_set()) else {
      continue;
    };
    let Some(a) = c
      .get_arguments()
      .find(|a| a.get_id() == *arg && !a.is_hide_set())
    else {
      continue;
    };
    let v: Vec<String> = a
      .get_possible_values()
      .iter()
      .map(|p| format!("`{}`", p.get_name()))
      .collect();
    if !v.is_empty() {
      values.push_str(&format!("- `{sub} {flag}`: {}\n", v.join(", ")));
    }
  }
  if !values.is_empty() {
    out.push_str("\n## Option values\n\n");
    out.push_str(&values);
  }

  out.push_str("\n## Session\n\n");
  let mut ways = Vec::new();
  if info.supports(Cap::QrLogin) {
    ways.push(format!("`media {} login` (QR code)", info.id));
  }
  if cfg!(feature = "browser") {
    ways.push(format!("`media {} login --browser`", info.id));
  }
  ways.push(format!("`media {} login --cookie '...'`", info.id));
  let ways = match ways.split_last() {
    Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
    _ => ways.concat(),
  };
  out.push_str(&format!("Log in with {ways}. "));
  if !info.required_cookies.is_empty() {
    let names: Vec<String> = info
      .required_cookies
      .iter()
      .map(|c| format!("`{c}`"))
      .collect();
    let noun = if names.len() > 1 { "cookies" } else { "cookie" };
    out.push_str(&format!(
      "A session needs the {noun} {}. ",
      names.join(" and ")
    ));
  }
  out.push_str(&format!(
    "`MEDIA_{}_COOKIE` supplies a cookie header for one run.\n\n",
    info.id.to_ascii_uppercase()
  ));
  out.push_str(info.guide.trim());
  out.push('\n');
  out
}

/// The index of `media guide`.
pub fn index(platforms: &[(&'static str, &'static str)]) -> String {
  let mut out =
    String::from("# media-cli guide\n\nRead a topic with `media guide <topic>`.\n\n## Topics\n\n");
  for t in TOPICS {
    out.push_str(&format!("- `{}`: {}\n", t.name, t.summary));
  }
  out.push_str("\n## Platforms\n\n");
  for (id, name) in platforms {
    out.push_str(&format!(
      "- `{id}`: {name}: commands, references, login, analytics, limits\n"
    ));
  }
  out
}

/// Print Markdown: lightly styled on a terminal, verbatim otherwise.
pub fn print(markdown: &str) {
  if !std::io::stdout().is_terminal() {
    print!("{markdown}");
    return;
  }
  let mut fenced = false;
  for line in markdown.lines() {
    if line.starts_with("```") {
      fenced = !fenced;
      continue;
    }
    if fenced || line.starts_with("    ") {
      let code = line.strip_prefix("    ").unwrap_or(line);
      println!(
        "    {}",
        code.if_supports_color(Stream::Stdout, |t| t.cyan())
      );
    } else if let Some(h) = line.strip_prefix("# ").map(|h| h.replace('`', "")) {
      println!(
        "{}",
        h.if_supports_color(Stream::Stdout, |t| t.bold())
          .if_supports_color(Stream::Stdout, |t| t.underline())
      );
    } else if let Some(h) = line
      .strip_prefix("## ")
      .or_else(|| line.strip_prefix("### "))
      .map(|h| h.replace('`', ""))
    {
      println!("{}", h.if_supports_color(Stream::Stdout, |t| t.bold()));
    } else {
      println!("{}", inline(line));
    }
  }
}

/// `code` in cyan and **bold** in bold; the markers themselves are dropped.
fn inline(line: &str) -> String {
  let mut out = String::new();
  for (i, part) in line.split('`').enumerate() {
    if i % 2 == 1 {
      out.push_str(
        &part
          .if_supports_color(Stream::Stdout, |t| t.cyan())
          .to_string(),
      );
      continue;
    }
    for (j, bit) in part.split("**").enumerate() {
      if j % 2 == 1 {
        out.push_str(
          &bit
            .if_supports_color(Stream::Stdout, |t| t.bold())
            .to_string(),
        );
      } else {
        out.push_str(bit);
      }
    }
  }
  out
}
