## References

- Tweets: the numeric id or a status link (`x.com/<user>/status/<id>`,
  `twitter.com/…`, with or without `https://`, `/photo/1` suffixes too).
- Users: `NASA`, `@NASA`, `https://x.com/NASA`, or the numeric id.
- Lists: the id or an `x.com/i/lists/<id>` link.

## Logging in

X has no QR login: use `media x login --browser`, or `--cookie` with at
least `auth_token` and `ct0` (the full cookie header avoids "automated
behaviour" errors, code 226). Without a login, `user`, `read`,
`user-posts` and `download` work through a guest token.

## Analytics

- `media x insights [--days N]`: X shows account analytics (daily
  impressions, engagements, profile visits, follows, audience, top posts)
  to X Premium only. Without Premium you get a warning plus X's rollup of
  the last 8 days and numbers summed from your own posts in the window.
- `media x insights <your tweet>`: lifetime analytics without Premium
  (impressions, engagements, likes, replies, retweets, bookmarks, profile
  visits, link / media / detail clicks, new follows); daily series and
  audience need Premium. Others' tweets get public counters.

## Interactions

- `likers` works only on your own tweets (likes are private since 2024).
- `reposts` lists quote tweets; `retweeters TWEET` lists who retweeted.
- `retweet TWEET [--undo]`, `quote TWEET "text" [-i IMAGE]`.
- `comment TWEET "text" [-i IMAGE ...]` replies; `--reply-to` answers one
  of its replies.
- `post "text" [-i IMAGE ...]` (up to four images), with `--reply-to` or
  `--quote`; `delete` and `delete-comment` delete your tweets.
- `favorite` bookmarks; `folders` lists bookmark folders for
  `favorites --folder`.
- `hot --category trending|for-you|news|sports|entertainment` reads trends
  (their `url` is a search, not a tweet).

Writes wait 1.5–4 s first, as X flags bursts.

## Downloads

Photos at original size (`name=orig`), videos and GIFs as the best-bitrate MP4.
