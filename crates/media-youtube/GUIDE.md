## References

- Videos: the 11-character id, or `watch?v=`, `youtu.be/`, `/shorts/`,
  `/live/` and `/embed/` links; community posts by link.
- Channels: `@handle`, the `UC…` id, or `/channel/`, `/c/`, `/user/`, `/@`
  links.
- Playlists: the id or a `list=` link.

## Logging in

`media yt login --browser`. Reads work without an account; your feed,
subscriptions, library, history, notifications and every write need one.

## Analytics

YouTube's creator analytics come from the official YouTube Analytics API,
which needs a one-time setup:

1. In Google Cloud Console create a project and enable the **YouTube
   Analytics API** and the **YouTube Data API v3**.
2. Create an OAuth client of type **Desktop app**.
3. `export YOUTUBE_CLIENT_ID=… YOUTUBE_CLIENT_SECRET=…`, run
   `media yt oauth`, allow access in the browser, and export the
   `YOUTUBE_REFRESH_TOKEN` it prints.

Then `media yt insights [--days N]` and `media yt insights <your video>`
report views, watch minutes, average view duration and percentage,
subscribers gained / lost, likes, dislikes, comments, shares, playlist adds,
daily series, and breakdowns by traffic source, playback location, device,
operating system, subscription status, country, sharing service, age and
gender (plus top videos for the account). Impressions and click-through
rate are not in the API. Without the setup you get channel totals and the
views of your recent uploads, with a warning.

## Browsing

- `search` sorts by `relevance` or `popularity` (YouTube dropped the date
  and rating orders); `--filter` narrows by type, upload date, length and
  features (values below). `search -t topic` finds playlists.
- `hot` reads the Hype leaderboard (Trending is gone) or an Explore page
  (`--category music|gaming|news|sports|live|learning|podcasts`); it follows
  your IP's region.
- `transcript VIDEO [--lang L]`, `playlist ID`, `shorts CHANNEL`,
  `streams CHANNEL`, `community CHANNEL`, `related VIDEO`, `hashtag TAG`.

## Interactions

`like` / `dislike` (only you see dislikes), `favorite` (Watch later, or
`--folder` a playlist id), `comment` (+ `--reply-to`), `delete-comment`,
`follow` (subscribe). Uploads and community posts are not supported.

## Downloads

The best streams come from YouTube's Android VR client in 8 MB ranges and
are merged with ffmpeg. Videos that need a proof-of-origin token (the
stream stops after 1 MB) are handed to `yt-dlp` when it is installed, for
plain https formats. Live streams can be downloaded once they end.
