## References

- Posts: the id (`1wt65ox`), `t3_<id>`, links on www / old / np.reddit.com,
  `redd.it/<id>`, `/gallery/<id>`, `v.redd.it/<id>` and share links.
- Comments: `t1_<id>` or a comment permalink.
- Subreddits: `rust`, `r/rust` or a link; users: `spez`, `u/spez` or a link.

## Logging in

`media reddit login --browser` (the `reddit_session` cookie). Writes use the
web app's own `token_v2` bearer, renewed automatically. Alternatively set
`REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME` and
`REDDIT_PASSWORD` for a Reddit "script" app; that switches everything to
Reddit's OAuth API.

Reddit refuses anonymous API requests from many networks; log in (or use
`--proxy`) when commands answer "blocked by network security".

## Analytics

- `media reddit insights [--days N]`: karma (post, comment, awarder,
  awardee) with per-subreddit breakdowns, followers, account age and
  trophies, plus daily numbers computed from your own posts and comments
  created in the window (their current scores, counted on their creation
  day). `extra.native` / `extra.computed` say which is which.
- `media reddit insights <post>`: public counters (score, upvote ratio,
  comments, crossposts, awards, views when sent); for your own posts also
  the author-only post insights page.

## Interactions

- `reposts` lists crossposts; there is no `likers` (votes are private).
- `comments --all` expands every "load more" and "continue this thread".
- `sub SUBREDDIT [--sort hot|new|top|rising|controversial] [--time …]`
  browses a subreddit; `subreddit NAME` shows its details; `subreddits
  [popular|new|default]`.
- `post "text" --title T --topic r/<sub>` (or `u/<you>` for your profile):
  a single URL as text makes a link post, `-i` images an image or gallery
  post, `--quote POST` a crosspost.
- `downvote THING [--undo]`, `edit THING "text"`, `follow r/<sub>` or
  `follow u/<user>`, `mark-read` (marks the whole inbox read).
- `user-comments USER` lists a user's comments.
