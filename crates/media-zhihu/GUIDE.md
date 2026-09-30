## References

- Links: `https://www.zhihu.com/question/<q>`, `…/question/<q>/answer/<a>`,
  `https://www.zhihu.com/answer/<a>`, `https://zhuanlan.zhihu.com/p/<id>`
  (article), `https://www.zhihu.com/pin/<id>` (pin, 想法).
- Typed ids: `q:<id>`, `a:<id>`, `article:<id>`, `p:<id>` (pin); a bare
  number is an answer.
- Users: the url_token (`zhang-jia-wei`), `@token`, or a `/people/` link.

## Logging in

`media zhihu login` shows a QR code for the Zhihu app. Zhihu refuses almost
everything to anonymous clients (only `hot` works) and flags networks that
keep trying; when the QR poll reports risk control, pass the check in the
browser or use `media zhihu login --browser`.

## Analytics

`media zhihu insights [--days N]`: the creator center.

- totals and daily series: `views`, `impressions`, `ctr`, `completion_rate`,
  `likes` (赞同), `hearts` (喜欢), `reactions`, `comments`, `favorites`,
  `shares`, `reposts`, follower gains and losses, `profile_visitors`.
- breakdowns: `traffic_source`, reader `gender` / `age` / `region` (lifetime),
  `content_type`, and a follower portrait once Zhihu builds one.

`media zhihu insights <your answer, article or pin>`: the same for one post
plus followers it brought. Others' posts get public counters.

`media zhihu creations -t answer|article|pin` lists your posts with their
lifetime numbers.

## Interactions

- `likers` lists upvoters of answers, articles and pins.
- `answers QUESTION [--sort default|created]`, `topic ID [--essence]`,
  `user-articles USER`, `user-pins USER`.
- `post` publishes a pin (想法) with optional `--title` and images; `ask
  TITLE [-d DETAIL] [-t TOPIC] [-i IMAGE]` asks a question.
- `answer QUESTION BODY [-m] [-i IMAGE]` answers a question and `article
  TITLE BODY [-m] [-t TOPIC] [-i IMAGE]` publishes a column article; `-` as
  BODY reads it from stdin: `media zhihu answer q:123 - -m < answer.md`.
- `follow-question QUESTION [--undo]`.
- `delete` removes your own pin, article, question or answer.

With `-m / --markdown` the body is read as Markdown: `#` / `##` headings
(deeper levels become bold), `-` and `1.` lists, `>` quotes, fenced code,
`|` tables, `---` rules, `**bold**`, `` `code` ``, `[text](url)` links, an
image alone on its line (`![caption](path)`, uploaded and placed there), and
a link alone on its line with the title `"card"` (`[text](url "card")`) as a
link card. Without it, every line becomes a paragraph.

## Limits

Questions and topics are read from their pages (the API needs a signature).
Reposts have no public list.
