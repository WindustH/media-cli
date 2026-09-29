//! Xiaohongshu-only commands.

use media_core::{Data, Result};

use crate::api::Client;
use crate::creator;

#[derive(Debug, clap::Subcommand)]
pub enum XhsCommand {
  /// Your own published notes (creator center)
  MyNotes {
    /// Page number, starting at 0
    #[arg(long, default_value_t = 0)]
    page: u32,
  },
}

pub async fn run(c: &Client, command: XhsCommand) -> Result<Data> {
  match command {
    XhsCommand::MyNotes { page } => Ok(Data::Posts(creator::my_notes(c, page).await?)),
  }
}
