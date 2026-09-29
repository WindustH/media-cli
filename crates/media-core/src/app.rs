//! The top-level program: a registry of platforms behind one `media` command.
//!
//! The binary only lists the platforms; everything else (argument parsing,
//! runtime, output) happens here. Invoking the binary through a symlink named
//! after a platform (`bili`, `xhs`, ...) selects that platform directly.

use std::ffi::OsString;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::ExitCode;

use clap::{ArgMatches, Args, FromArgMatches};
use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Table, presets};
use serde_json::json;

use crate::cli::{self, GlobalArgs};
use crate::model::Data;
use crate::output::{self, Format};
use crate::platform::{Cap, Platform, PlatformInfo};

type RunFuture = Pin<Box<dyn Future<Output = ExitCode>>>;

struct Entry {
  info: PlatformInfo,
  command: fn() -> clap::Command,
  extras: fn() -> Vec<String>,
  run: fn(GlobalArgs, ArgMatches) -> RunFuture,
}

pub struct App {
  name: &'static str,
  version: &'static str,
  about: &'static str,
  platforms: Vec<Entry>,
}

fn run_boxed<P: Platform + 'static>(global: GlobalArgs, matches: ArgMatches) -> RunFuture {
  Box::pin(cli::run::<P>(global, matches))
}

impl App {
  pub fn new(name: &'static str, version: &'static str, about: &'static str) -> Self {
    Self {
      name,
      version,
      about,
      platforms: Vec::new(),
    }
  }

  pub fn platform<P: Platform + 'static>(mut self) -> Self {
    self.platforms.push(Entry {
      info: P::INFO,
      command: cli::command::<P>,
      extras: cli::extra_commands::<P>,
      run: run_boxed::<P>,
    });
    self
  }

  fn find(&self, name: &str) -> Option<&Entry> {
    self
      .platforms
      .iter()
      .find(|e| e.info.id == name || e.info.aliases.contains(&name))
  }

  fn command(&self) -> clap::Command {
    let mut cmd = GlobalArgs::augment_args(clap::Command::new(self.name))
      .version(self.version)
      .about(self.about)
      .long_about(None)
      .subcommand_required(true)
      .arg_required_else_help(true)
      .subcommand(
        clap::Command::new("platforms").about("List platforms and what each one supports"),
      );
    for e in &self.platforms {
      cmd = cmd.subcommand((e.command)());
    }
    cmd
  }

  pub fn run(self) -> ExitCode {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    let invoked = args
      .first()
      .and_then(|a| Path::new(a).file_stem())
      .and_then(|s| s.to_str())
      .map(str::to_owned);
    if let Some(entry) = invoked.as_deref().and_then(|n| self.find(n)) {
      args.insert(1, entry.info.id.into());
    }
    let matches = match self.command().try_get_matches_from(args) {
      Ok(m) => m,
      Err(e) => e.exit(),
    };
    let global = match GlobalArgs::from_arg_matches(&matches) {
      Ok(g) => g,
      Err(e) => e.exit(),
    };
    init_logging(global.verbose);

    let Some((name, sub)) = matches.subcommand() else {
      return ExitCode::FAILURE;
    };
    if name == "platforms" {
      self.print_platforms(global.output_format());
      return ExitCode::SUCCESS;
    }
    let Some(entry) = self.find(name) else {
      return ExitCode::FAILURE;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
    {
      Ok(rt) => rt,
      Err(e) => {
        eprintln!("error: cannot start the async runtime: {e}");
        return ExitCode::FAILURE;
      }
    };
    runtime.block_on((entry.run)(global, sub.clone()))
  }

  fn print_platforms(&self, format: Format) {
    if format != Format::Table {
      let list: Vec<_> = self
        .platforms
        .iter()
        .map(|e| {
          json!({
            "id": e.info.id,
            "name": e.info.name,
            "aliases": e.info.aliases,
            "home": e.info.home,
            "capabilities": e.info.caps.iter().map(|c| c.name()).collect::<Vec<_>>(),
            "extra_commands": (e.extras)(),
          })
        })
        .collect();
      output::emit(format, None, &Data::Value(json!({ "platforms": list })));
      return;
    }
    let mut t = Table::new();
    t.load_style(presets::NOTHING)
      .set_content_arrangement(ContentArrangement::Dynamic);
    let mut header = vec![Cell::new("")];
    header.extend(
      self
        .platforms
        .iter()
        .map(|e| Cell::new(e.info.id).add_attribute(Attribute::Bold)),
    );
    t.set_header(header);
    for cap in Cap::ALL {
      let mut row = vec![Cell::new(cap.name())];
      row.extend(self.platforms.iter().map(|e| {
        Cell::new(if e.info.supports(*cap) { "✓" } else { "·" })
          .set_alignment(CellAlignment::Center)
      }));
      t.add_row(row);
    }
    println!("{t}");
    for e in &self.platforms {
      let extras = (e.extras)();
      if !extras.is_empty() {
        println!(
          "\n{} ({}) only: {}",
          e.info.name,
          e.info.id,
          extras.join(", ")
        );
      }
    }
  }
}

fn init_logging(verbose: bool) {
  let filter = tracing_subscriber::EnvFilter::try_from_env("MEDIA_LOG").unwrap_or_else(|_| {
    tracing_subscriber::EnvFilter::new(if verbose { "warn,media=debug" } else { "warn" })
  });
  let _ = tracing_subscriber::fmt()
    .with_env_filter(filter)
    .with_writer(std::io::stderr)
    .without_time()
    .try_init();
}
