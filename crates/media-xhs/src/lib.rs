//! Xiaohongshu for media-cli.

use media_core::{Ctx, Data, NoExtra, Platform, PlatformInfo, Result, User};

pub struct Xhs {
  ctx: Ctx,
}

impl Platform for Xhs {
  const INFO: PlatformInfo = PlatformInfo {
    id: "xhs",
    name: "Xiaohongshu",
    aliases: &[],
    about: "Xiaohongshu / RedNote (小红书)",
    home: "https://www.xiaohongshu.com",
    cookie_domains: &[],
    required_cookies: &[],
    caps: &[],
    choices: media_core::Choices::NONE,
    min_interval: std::time::Duration::ZERO,
  };

  type Extra = NoExtra;

  fn new(ctx: Ctx) -> Result<Self> {
    Ok(Self { ctx })
  }

  fn ctx(&self) -> &Ctx {
    &self.ctx
  }

  async fn whoami(&self) -> Result<User> {
    Err(media_core::Error::unsupported("whoami"))
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    match command {}
  }
}
