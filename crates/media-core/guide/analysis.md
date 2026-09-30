# Data analysis

## Creator analytics: `insights`

    media <platform> insights [--days N]         # your account
    media <platform> insights POST [--days N]    # one of your posts

The result has:

- `totals`: headline numbers (`views`, `impressions`, `likes`, `comments`,
  `shares`, `favorites`, `new_followers`, `lost_followers`, `net_followers`,
  `avg_watch_seconds`, `completion_rate`, `ctr` ...). Rates are fractions
  (0..1). `totals_period` says when they are not the `from`..`to` window
  (for a post usually `lifetime`).
- `series`: one daily trend per metric.
- `breakdowns`: distributions such as `traffic_source`, `device`, `gender`,
  `age`, `region`, `interest`; `period` says what each covers.
- `warnings`: why something is missing (a paid tier, a follower threshold, a
  data center not enabled yet, a panel that failed).

Analytics exist only for your own account and posts. For anyone else's post
`insights` returns its public counters, with a warning.

| Platform | Account | Your posts | Notes |
| --- | --- | --- | --- |
| Bilibili | ✓ | ✓ videos | windows: yesterday, 7, 30, 90 days or all |
| Zhihu | ✓ | ✓ answers, articles, pins | portraits are lifetime |
| Xiaohongshu | ✓ | ✓ notes | 7 or 30 days; portraits from 50 fans |
| Twitter / X | Premium | ✓ free | without Premium: 8-day rollup + your posts |
| Reddit | karma + computed | public counters + own post insights | |
| YouTube | OAuth | OAuth | needs a Google Cloud OAuth client |

Each platform's guide (`media guide <platform>`) lists the exact metrics.

## Interactions

    media <platform> likers POST        # who liked it (Zhihu, Bilibili, X own posts)
    media <platform> reposts POST       # reposts / quotes / crossposts (Bilibili, X, Reddit)
    media <platform> comments POST --all --replies    # the complete thread

## Exporting

    media bili user-posts 946974 -n 500 --since 90d -f csv > videos.csv
    media bili comments BV1xx411c7mD --all --replies -f jsonl > comments.jsonl
    media zhihu insights --days 30 -f csv > zhihu_account.csv
    media bili video-stats -f csv > my_videos.csv

In CSV, insights rows carry `section` (`total`, `series`, `breakdown`,
`warning`), `metric`, `date`, `dimension`, `label`, `value`, `ratio`,
`period`; comments carry `depth` and `parent_id`.

## Time series by snapshot

Every row carries `fetched_at`, so appending snapshots builds a history of
numbers the platforms do not keep, e.g. hourly with cron:

    0 * * * * media bili user-posts 946974 -n 30 -f jsonl >> ~/data/bili.jsonl

Load with pandas (`pd.read_json(path, lines=True)`) or DuckDB
(`SELECT * FROM read_json_auto('bili.jsonl')`).

## Being polite

Platforms watch for automation. For large jobs slow down:

    media --interval 5 bili comments BV1xx411c7mD --all --replies -f jsonl

`--interval SECS` (or `MEDIA_INTERVAL`) sets a minimum gap between requests,
never below the platform's own pacing; random jitter is added. Stop when a
command answers `verification_required` or `rate_limited`.
