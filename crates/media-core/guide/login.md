# Logging in

A session is a set of cookies (plus, for some platforms, tokens) saved per
platform. Every command uses it automatically.

## Ways to log in

    media <platform> login              # QR code where supported, else the browser
    media <platform> login --qrcode     # scan with the platform's mobile app
    media <platform> login --browser    # read the session from local browsers
    media <platform> login --browser firefox
    media <platform> login --cookie 'name=value; name2=value2'

- **QR code** (Zhihu, Xiaohongshu, Bilibili): the code is drawn in the
  terminal and saved as an SVG under the cache directory; scan it with the
  app and confirm. Xiaohongshu may ask the new device for a captcha after the
  scan; use `--browser` there.
- **Browser**: reads the cookies of a browser where you are logged in
  (Chrome, Chromium, Edge, Brave, Firefox, LibreWolf, Zen, Vivaldi, Opera,
  Arc). Only the platform's own domains are read. Log in on the website first.
- **Cookie string**: copy the `Cookie` request header from the browser's
  developer tools (Network tab, any request to the site).

`login` verifies the session with `whoami` before saving it.

## Checking and removing sessions

    media <platform> status             # exit code 0 when logged in, 1 when not
    media <platform> whoami             # the account
    media <platform> logout             # delete the saved session

## Without saving

- `MEDIA_<PLATFORM>_COOKIE`, with the platform's name rather than an alias
  (`MEDIA_BILI_COOKIE`, `MEDIA_TWITTER_COOKIE`), supplies a cookie header for
  one run; it wins over the saved session and is never written.
- Reddit: `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME`,
  `REDDIT_PASSWORD` switch to Reddit's OAuth API (a "script" app).
- YouTube analytics: `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`,
  `YOUTUBE_REFRESH_TOKEN` (see `media guide youtube`).

## Where sessions live

`~/.config/media-cli/<platform>/session.json`, readable by you only.
`MEDIA_CLI_HOME=<dir>` moves config and cache under `<dir>`. Only logged-in
sessions are saved; cookies a platform refreshes are written back.

## When a platform pushes back

| Error code | Meaning | What to do |
| --- | --- | --- |
| `not_authenticated` | no session, or it expired | log in again |
| `verification_required` | captcha / risk control | pass the check in the browser, wait, or change network (`--proxy`) |
| `rate_limited` | too many requests | wait; slow down with `--interval` |
| `ip_blocked` | the network is blocked | change network |
