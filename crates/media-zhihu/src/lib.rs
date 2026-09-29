//! Zhihu for media-cli.

use media_core::{Ctx, Data, NoExtra, Platform, PlatformInfo, Result, User};

pub struct Zhihu {
  ctx: Ctx,
}

impl Platform for Zhihu {
  const INFO: PlatformInfo = PlatformInfo {
    id: "zhihu",
    name: "Zhihu",
    aliases: &[],
    about: "Zhihu (知乎)",
    home: "https://www.zhihu.com",
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
