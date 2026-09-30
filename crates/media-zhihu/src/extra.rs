//! Zhihu-only commands.

use std::path::PathBuf;

use media_core::cli::PageArgs;
use media_core::{Ctx, Data, Error, Page, Result};

use crate::refs::{self, Target};
use crate::{insights, people, publish, read, write};

#[derive(Debug, clap::Subcommand)]
pub enum Extra {
  /// Answers of a question
  Answers {
    /// Question id, q:<id> or URL
    question: String,
    /// Answer order
    #[arg(long, default_value = "default", value_parser = ["default", "created"])]
    sort: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Ask a question (提问)
  Ask {
    title: String,
    /// Question description
    #[arg(short, long, default_value = "")]
    detail: String,
    /// Topic id (repeatable)
    #[arg(short = 't', long = "topic", value_name = "TOPIC")]
    topics: Vec<String>,
    /// Attach an image (repeatable)
    #[arg(short = 'i', long = "image", value_name = "PATH")]
    images: Vec<PathBuf>,
  },
  /// Answer a question (回答)
  Answer {
    /// Question id, q:<id> or URL
    question: String,
    /// Body text; `-` reads it from stdin
    body: String,
    /// Read the body as Markdown (headings, lists, quotes, code, tables, inline images)
    #[arg(short, long)]
    markdown: bool,
    /// Attach an image at the end (repeatable)
    #[arg(short = 'i', long = "image", value_name = "PATH")]
    images: Vec<PathBuf>,
  },
  /// Publish a column article (文章)
  Article {
    title: String,
    /// Body text; `-` reads it from stdin
    body: String,
    /// Read the body as Markdown (headings, lists, quotes, code, tables, inline images)
    #[arg(short, long)]
    markdown: bool,
    /// Topic id (repeatable)
    #[arg(short = 't', long = "topic", value_name = "TOPIC")]
    topics: Vec<String>,
    /// Attach an image (repeatable)
    #[arg(short = 'i', long = "image", value_name = "PATH")]
    images: Vec<PathBuf>,
  },
  /// Follow a question
  FollowQuestion {
    /// Question id, q:<id> or URL
    question: String,
    /// Unfollow instead
    #[arg(long)]
    undo: bool,
  },
  /// Show a topic, or list its essence (精华) posts
  Topic {
    id: String,
    /// List the topic's essence posts instead
    #[arg(long)]
    essence: bool,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Articles of a user
  UserArticles {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Pins (想法) of a user
  UserPins {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your own posts with their lifetime numbers from the creator center (内容分析)
  Creations {
    /// Content type
    #[arg(short = 't', long = "type", default_value = "answer", value_parser = ["answer", "article", "pin"])]
    kind: String,
    #[command(flatten)]
    page: PageArgs,
  },
}

pub async fn run(ctx: &Ctx, command: Extra) -> Result<Data> {
  match command {
    Extra::Answers {
      question,
      sort,
      page,
    } => {
      let id = Target::question(&ctx.post_ref(&question)?)?;
      let posts = page.collect_dated(async |r| read::answers(ctx, &id, &sort, &r).await);
      Ok(Data::Posts(posts.await?))
    }
    Extra::Ask {
      title,
      detail,
      topics,
      images,
    } => {
      check(&title, &images)?;
      let done = publish::question(ctx, title.trim(), &detail, &topics, &images);
      Ok(Data::Action(done.await?))
    }
    Extra::Answer {
      question,
      body,
      markdown,
      images,
    } => {
      let id = Target::question(&ctx.post_ref(&question)?)?;
      let body = read_body(body, "the answer")?;
      check("answer", &images)?;
      let done = publish::answer(ctx, &id, &body, markdown, &images);
      Ok(Data::Action(done.await?))
    }
    Extra::Article {
      title,
      body,
      markdown,
      topics,
      images,
    } => {
      let body = read_body(body, "the article body")?;
      check(&title, &images)?;
      let done = publish::article(ctx, title.trim(), &body, markdown, &topics, &images);
      Ok(Data::Action(done.await?))
    }
    Extra::FollowQuestion { question, undo } => {
      let id = Target::question(&ctx.post_ref(&question)?)?;
      Ok(Data::Action(
        write::follow(ctx, "questions", &id, undo).await?,
      ))
    }
    Extra::Topic { id, essence, page } => {
      let id = topic_id(&id)?;
      if essence {
        let posts = page.collect_dated(async |r| read::topic_essence(ctx, &id, &r).await);
        Ok(Data::Posts(posts.await?))
      } else {
        let topic = read::topic(ctx, &id).await?;
        Ok(Data::Collections(Page::last(vec![topic])))
      }
    }
    Extra::UserArticles { user, page } => {
      let token = refs::user(&ctx.user_ref(&user)?)?;
      let posts = page.collect_dated(async |r| people::articles(ctx, &token, &r).await);
      Ok(Data::Posts(posts.await?))
    }
    Extra::UserPins { user, page } => {
      let token = refs::user(&ctx.user_ref(&user)?)?;
      let posts = page.collect_dated(async |r| people::pins(ctx, &token, &r).await);
      Ok(Data::Posts(posts.await?))
    }
    Extra::Creations { kind, page } => {
      let posts = page.collect_dated(async |r| insights::creations(ctx, &kind, &r).await);
      Ok(Data::Posts(posts.await?))
    }
  }
}

/// The body argument, or standard input for `-`; empty bodies are refused.
fn read_body(body: String, what: &str) -> Result<String> {
  let body = if body == "-" {
    std::io::read_to_string(std::io::stdin())?
  } else {
    body
  };
  if body.trim().is_empty() {
    return Err(Error::input(format!("{what} is empty")));
  }
  Ok(body)
}

fn check(title: &str, images: &[PathBuf]) -> Result<()> {
  if title.trim().is_empty() {
    return Err(Error::input("the title is empty"));
  }
  match images.iter().find(|i| !i.is_file()) {
    Some(missing) => Err(Error::input(format!(
      "image not found: {}",
      missing.display()
    ))),
    None => Ok(()),
  }
}

/// A topic id, bare or from `https://www.zhihu.com/topic/<id>`.
fn topic_id(arg: &str) -> Result<String> {
  let id = arg
    .trim()
    .trim_end_matches('/')
    .rsplit("/topic/")
    .next()
    .unwrap_or_default()
    .split(['/', '?', '#'])
    .next()
    .unwrap_or_default();
  if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
    Ok(id.to_owned())
  } else {
    Err(Error::input(format!("not a Zhihu topic: `{arg}`")))
  }
}
