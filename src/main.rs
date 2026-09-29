use std::process::ExitCode;

fn main() -> ExitCode {
  media_core::App::new(
    "media",
    env!("CARGO_PKG_VERSION"),
    env!("CARGO_PKG_DESCRIPTION"),
  )
  .platform::<media_zhihu::Zhihu>()
  .platform::<media_xhs::Xhs>()
  .platform::<media_twitter::Twitter>()
  .platform::<media_bilibili::Bilibili>()
  .run()
}
