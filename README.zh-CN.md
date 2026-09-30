<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/media-cli-logo-dark.svg">
    <img src="docs/assets/media-cli-logo-light.svg" alt="media-cli" width="360">
  </picture>
</p>

<p align="center">
  <strong>一个命令行，玩转知乎、小红书、Twitter / X、哔哩哔哩、Reddit 和 YouTube。</strong>
</p>

<p align="center">
  <a href="docs/output.md">输出格式</a> ·
  <a href="docs/architecture.md">架构</a> ·
  <a href="README.md">English</a>
</p>

---

`media` 用同一套命令在六个社交平台上浏览、搜索、发布和下载。在终端里给人看，
在脚本和 AI Agent 里则输出稳定的 JSON / YAML。

```console
$ media bili search "rust 教程" -n 3
 #  Content                                                      Author        Likes  Cmts  Views   Time
 1  Rust编程语言入门教程（Rust语言/Rust权威指南配套）【已完结】  软件工艺师    26.1k  2.6k    1.7M  2020-10-21
 2  【Rust腐蚀 新手教程】2026保姆级教程 实时更新！！！           Utaoki丶苦药   1.9k   401  155.3k  2025-01-13
 3  Rust语言 Slint教程，非tauri                                  猩猩程序员        0     0    7.5k

$ media bili read '#1'
$ media bili download '#1' --audio-only
```

## 为什么用 media-cli

- **学一次，到处用。** `search`、`hot`、`feed`、`read`、`comments`、`user`、
  `like`、`favorite`、`comment`、`follow`、`post`、`download` 等命令在每个平台
  含义相同。`media platforms` 列出各平台支持的功能。
- **为 Agent 和脚本设计。** 所有结果都装在同一个信封里，字段和错误码稳定；
  输出被管道接走时自动切换为 YAML。帖子、用户、评论在各平台是同一种结构。
- **直接用于数据分析。** 创作者后台数据（`insights`：趋势、流量来源、观众画像）、
  点赞和转发的人、完整的评论楼中楼、`--since` / `--until` 时间过滤，以及可直接
  导入 pandas / DuckDB 的 CSV、JSON Lines 输出，每行都带抓取时间。
- **短引用。** 任何列表之后，`#3` 就是「第三条」，不用复制长长的 id 或链接。
- **翻页不丢位置。** `-n 200` 自动翻页；`--cursor` 从上次停下的地方精确继续。
- **登录省事。** 用知乎、小红书或 B 站 App 扫码，沿用浏览器里的登录状态，或粘贴 Cookie。
  登录状态只保存在本机。
- **很多功能无需账号。** B 站的视频、评论、用户和排行榜；Twitter 的用户资料、
  推文和时间线；小红书笔记；Reddit 的帖子、评论、社区和用户；YouTube 的视频、
  Shorts、频道、评论、播放列表、字幕和下载。
- **自带下载。** 原图、合并好音轨的视频、纯音频，或可直接用于语音识别的 WAV 分段。
- **像浏览器一样访问。** Chrome 的网络指纹、各平台的请求签名和克制的请求节奏，
  让请求看起来和普通浏览无异。
- **平台特色功能。** B 站投币、一键三连、字幕、AI 总结和弹幕；Twitter 转推、
  引用和列表；知乎提问、写文章、看回答；小红书创作者笔记；Reddit 社区浏览、
  踩、转帖和多图帖；YouTube 字幕文本、Shorts、直播、社区帖子、话题标签、播放列表和相关视频。

## 各平台支持的功能

| | 知乎 | 小红书 | Twitter / X | 哔哩哔哩 | Reddit | YouTube |
| --- | :-: | :-: | :-: | :-: | :-: | :-: |
| 扫码登录 | ✓ | ✓ | | ✓ | | |
| 搜索内容 / 用户 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 搜索话题 | ✓ | ✓ | | | ✓ | 播放列表 |
| 热门、推荐流 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 阅读、评论 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 楼中楼回复 | ✓ | ✓ | | ✓ | ✓ | ✓ |
| 用户资料、用户作品 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 粉丝、关注 | ✓ | | ✓ | ✓ | | 关注 |
| 收藏夹、收藏内容 | ✓ | 收藏内容 | ✓ | ✓ | ✓ | ✓ |
| 点赞过的内容 | | ✓ | ✓ | | ✓ | ✓ |
| 观看历史 | | | | ✓ | | ✓ |
| 通知 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 点赞、收藏、评论、关注 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 发布、删除 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 下载 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## 快速开始

从源码构建（需要 Rust 1.88+，以及构建 TLS 库所需的 `cmake` 和 `clang`；
`ffmpeg` 可选，用于合并视频和音频）：

```sh
cargo install --path .
```

不登录先试试：

```sh
media bili search "rust 教程" -n 5
media bili comments '#1'
media x user-posts NASA -n 10
media xhs read https://www.xiaohongshu.com/explore/68cfff45000000001003c253
media reddit sub rust --sort top --time week -n 5
media youtube search "rust tutorial" --filter video -n 5
media yt transcript https://youtu.be/5C_HPTJg5ek
```

登录后解锁全部功能：

```sh
media bili login                 # 用 B 站 App 扫码
media twitter login --cookie 'auth_token=...; ct0=...'
media reddit login --browser     # 沿用浏览器里的登录状态
media youtube login --browser
media zhihu status
```

更多例子：

```sh
media zhihu hot -n 10
media xhs search 咖啡 --sort popular
media x read https://x.com/NASA/status/2104695667180380260 --json
media bili subtitle BV1xx411c7mD
media bili download BV1xx411c7mD --audio-only --split 25
media twitter post "Hello" -i photo.jpg
media reddit comments '#1' --sort top
media reddit post "Hello from the terminal" --title "Hi" --topic r/test
media reddit follow r/rust
media youtube user @Fireship
media yt comments '#1' --sort new --all --replies -f csv > comments.csv
media youtube download https://www.youtube.com/shorts/fwBIZRq-vzY
```

每个平台还有更短的名字（`bilibili`、`x`、`xiaohongshu`、`zh`、`rd`、`yt` 等）。把程序链接成
`bili`、`xhs`、`twitter`、`zhihu`、`reddit` 或 `youtube`，就能省掉平台名：
`ln -s $(which media) ~/.local/bin/bili`。

## 文档

完整的使用文档内置在程序里：

```sh
media guide                 # 主题列表：start、login、refs、output、analysis、download ...
media guide analysis        # 创作者数据、导出、定时快照
media guide bili            # 单个平台：命令、引用格式、登录、限制
media bili search --help    # 单个命令的选项和示例
```

（内置文档为英文。）

## 须知

- `MEDIA_<平台>_COOKIE`（例如 `MEDIA_BILI_COOKIE`）可以临时提供 Cookie 而不保存。
  `--proxy` 或 `HTTPS_PROXY` 让请求走代理。
- `--interval 5`（或 `MEDIA_INTERVAL=5`）让请求之间至少间隔 5 秒，适合大批量采集。
- 登录状态保存在 `~/.config/media-cli/<平台>/session.json`（仅本人可读），
  `media <平台> logout` 即可删除。
- `login --browser` 从本机浏览器（Chrome、Edge、Firefox、Brave 等）读取登录状态。
  不需要这个功能时可用 `--no-default-features` 构建。
- Reddit 会拒绝部分网络的未登录请求。可以先登录，或者在
  <https://www.reddit.com/prefs/apps> 创建一个个人 "script" 应用，并设置
  `REDDIT_CLIENT_ID`、`REDDIT_CLIENT_SECRET`、`REDDIT_USERNAME` 和
  `REDDIT_PASSWORD`，改用 Reddit 官方 API。
- YouTube 的创作者数据（`media youtube insights`）来自官方 YouTube Analytics API：
  设置一个「桌面应用」OAuth 客户端的 `YOUTUBE_CLIENT_ID` 和 `YOUTUBE_CLIENT_SECRET`，
  运行一次 `media youtube oauth`，再设置它输出的 `YOUTUBE_REFRESH_TOKEN`。
- 批量操作请放慢节奏：各平台都会监测自动化行为，可能限制看起来像机器人的账号。

## 许可证

MIT
