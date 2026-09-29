//! GraphQL operations: fallback query ids, feature flags and how each is sent.
//!
//! Query ids rotate with web client releases; the ids here are fallbacks
//! (`web` refreshes them from the live client). Like the web client, queries
//! are GET and mutations POST: sent as POST, some queries (search, followers)
//! treat an expired session as anonymous and answer empty instead of 401.
//! Feature sets follow the reference client: GET sends only the enabled flags
//! (missing ones default to off upstream and long URLs are rejected), POST
//! sends the whole set.

use serde_json::{Map, Value};

pub type Flags = &'static [(&'static str, bool)];

/// One GraphQL operation.
#[derive(Debug, Clone, Copy)]
pub struct Op {
  pub name: &'static str,
  /// Fallback query id.
  pub id: &'static str,
  pub features: Flags,
  pub toggles: Flags,
  pub post: bool,
  /// Also works with a guest token (no login).
  pub guest: bool,
}

impl Op {
  const fn get(name: &'static str, id: &'static str, features: Flags) -> Self {
    Self {
      name,
      id,
      features,
      toggles: &[],
      post: false,
      guest: false,
    }
  }

  const fn post(name: &'static str, id: &'static str, features: Flags) -> Self {
    Self {
      post: true,
      ..Self::get(name, id, features)
    }
  }

  const fn guest(self) -> Self {
    Self {
      guest: true,
      ..self
    }
  }

  const fn toggles(self, toggles: Flags) -> Self {
    Self { toggles, ..self }
  }

  /// Feature object for the request (`compact`: enabled flags only).
  pub fn features(&self, compact: bool) -> Value {
    flags(self.features, compact)
  }

  pub fn field_toggles(&self) -> Option<Value> {
    (!self.toggles.is_empty()).then(|| flags(self.toggles, false))
  }
}

fn flags(set: Flags, compact: bool) -> Value {
  let map: Map<String, Value> = set
    .iter()
    .filter(|(_, on)| *on || !compact)
    .map(|(k, on)| ((*k).to_owned(), Value::Bool(*on)))
    .collect();
  Value::Object(map)
}

/// The reference client's default feature set.
const DEFAULT: Flags = &[
  ("responsive_web_graphql_exclude_directive_enabled", true),
  ("verified_phone_label_enabled", false),
  ("creator_subscriptions_tweet_preview_api_enabled", true),
  ("responsive_web_graphql_timeline_navigation_enabled", true),
  (
    "responsive_web_graphql_skip_user_profile_image_extensions_enabled",
    false,
  ),
  ("c9s_tweet_anatomy_moderator_badge_enabled", true),
  ("tweetypie_unmention_optimization_enabled", true),
  ("responsive_web_edit_tweet_api_enabled", true),
  (
    "graphql_is_translatable_rweb_tweet_is_translatable_enabled",
    true,
  ),
  ("view_counts_everywhere_api_enabled", true),
  ("longform_notetweets_consumption_enabled", true),
  (
    "responsive_web_twitter_article_tweet_consumption_enabled",
    true,
  ),
  ("tweet_awards_web_tipping_enabled", false),
  ("longform_notetweets_rich_text_read_enabled", true),
  ("longform_notetweets_inline_media_enabled", true),
  ("rweb_video_timestamps_enabled", true),
  ("responsive_web_media_download_video_enabled", true),
  ("freedom_of_speech_not_reach_fetch_enabled", true),
  ("standardized_nudges_misinfo", true),
  ("responsive_web_enhance_cards_enabled", false),
];

/// Default set plus article previews, for single tweets.
const TWEET: Flags = &[
  ("responsive_web_graphql_exclude_directive_enabled", true),
  ("creator_subscriptions_tweet_preview_api_enabled", true),
  ("responsive_web_graphql_timeline_navigation_enabled", true),
  ("c9s_tweet_anatomy_moderator_badge_enabled", true),
  ("tweetypie_unmention_optimization_enabled", true),
  ("responsive_web_edit_tweet_api_enabled", true),
  (
    "graphql_is_translatable_rweb_tweet_is_translatable_enabled",
    true,
  ),
  ("view_counts_everywhere_api_enabled", true),
  ("longform_notetweets_consumption_enabled", true),
  (
    "responsive_web_twitter_article_tweet_consumption_enabled",
    true,
  ),
  ("longform_notetweets_rich_text_read_enabled", true),
  ("longform_notetweets_inline_media_enabled", true),
  ("articles_preview_enabled", true),
  ("rweb_video_timestamps_enabled", true),
  ("freedom_of_speech_not_reach_fetch_enabled", true),
  ("standardized_nudges_misinfo", true),
];

/// The reference client's profile feature set.
const USER: Flags = &[
  ("hidden_profile_subscriptions_enabled", true),
  ("rweb_tipjar_consumption_enabled", true),
  ("responsive_web_graphql_exclude_directive_enabled", true),
  ("verified_phone_label_enabled", false),
  (
    "subscriptions_verification_info_is_identity_verified_enabled",
    true,
  ),
  (
    "subscriptions_verification_info_verified_since_enabled",
    true,
  ),
  ("highlights_tweets_tab_ui_enabled", true),
  ("responsive_web_twitter_article_notes_tab_enabled", true),
  ("subscriptions_feature_can_gift_premium", true),
  ("creator_subscriptions_tweet_preview_api_enabled", true),
  (
    "responsive_web_graphql_skip_user_profile_image_extensions_enabled",
    false,
  ),
  ("responsive_web_graphql_timeline_navigation_enabled", true),
];

/// The current web client's set for `CreateTweet` (the reference's default
/// set predates the live query id; mutations reject missing flags).
const CREATE: Flags = &[
  ("premium_content_api_read_enabled", false),
  ("communities_web_enable_tweet_community_results_fetch", true),
  ("c9s_tweet_anatomy_moderator_badge_enabled", true),
  (
    "responsive_web_grok_analyze_button_fetch_trends_enabled",
    false,
  ),
  ("responsive_web_grok_analyze_post_followups_enabled", true),
  ("rweb_cashtags_composer_attachment_enabled", true),
  ("responsive_web_jetfuel_frame", true),
  ("responsive_web_grok_share_attachment_enabled", true),
  ("responsive_web_grok_annotations_enabled", true),
  ("responsive_web_edit_tweet_api_enabled", true),
  ("rweb_conversational_replies_downvote_enabled", false),
  (
    "graphql_is_translatable_rweb_tweet_is_translatable_enabled",
    true,
  ),
  ("view_counts_everywhere_api_enabled", true),
  ("longform_notetweets_consumption_enabled", true),
  (
    "responsive_web_twitter_article_tweet_consumption_enabled",
    true,
  ),
  ("content_disclosure_indicator_enabled", true),
  ("content_disclosure_ai_generated_indicator_enabled", true),
  ("responsive_web_grok_show_grok_translated_post", true),
  ("responsive_web_grok_analysis_button_from_backend", true),
  ("post_ctas_fetch_enabled", true),
  ("longform_notetweets_rich_text_read_enabled", true),
  ("longform_notetweets_inline_media_enabled", false),
  ("profile_label_improvements_pcf_label_in_post_enabled", true),
  ("responsive_web_profile_redirect_enabled", false),
  ("rweb_tipjar_consumption_enabled", false),
  ("verified_phone_label_enabled", false),
  ("articles_preview_enabled", true),
  ("rweb_cashtags_enabled", true),
  (
    "responsive_web_grok_community_note_auto_translation_is_enabled",
    true,
  ),
  (
    "responsive_web_graphql_skip_user_profile_image_extensions_enabled",
    false,
  ),
  ("freedom_of_speech_not_reach_fetch_enabled", true),
  ("standardized_nudges_misinfo", true),
  (
    "tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled",
    true,
  ),
  ("responsive_web_grok_image_annotation_enabled", true),
  ("responsive_web_grok_imagine_annotation_enabled", true),
  ("responsive_web_graphql_timeline_navigation_enabled", true),
];

const NONE: Flags = &[];

// ── reading ───────────────────────────────────────────────────────────

pub const USER_BY_SCREEN_NAME: Op =
  Op::get("UserByScreenName", "KybxDj9RrADIITXlGG8kpw", USER).guest();
pub const USER_BY_REST_ID: Op = Op::get("UserByRestId", "IdmRdjYxIGI39Hdwkwo5cQ", USER).guest();
pub const TWEET_RESULT: Op = Op::get("TweetResultByRestId", "LbQZrAWyKPvExi8di3-EoA", TWEET)
  .toggles(&[
    ("withArticleRichContentState", false),
    ("withArticlePlainText", true),
  ])
  .guest();
pub const TWEET_DETAIL: Op = Op::get("TweetDetail", "blErEeZkos5TDrWmrCp7cw", DEFAULT).toggles(&[
  ("withArticleRichContentState", true),
  ("withArticlePlainText", false),
  ("withGrokAnalyze", false),
  ("withDisallowedReplyControls", false),
]);
pub const USER_TWEETS: Op = Op::get("UserTweets", "qJy3MbaNndtzxf9IqUzxMg", DEFAULT).guest();
pub const LIKES: Op = Op::get("Likes", "PgAssYGsPMMF1vVox5ysPg", DEFAULT);
pub const SEARCH: Op = Op::get("SearchTimeline", "uGB-gNd5HE4TkpO70OcFNw", DEFAULT);
pub const FOLLOWERS: Op = Op::get("Followers", "mrqxgX8JzwlL6pvYiC5CPA", DEFAULT);
pub const FOLLOWING: Op = Op::get("Following", "uwmIAx89XrXNuGY-Y7WFLg", DEFAULT);
pub const HOME: Op = Op::get("HomeTimeline", "7zlnp2TxC044W4C1ZUJMHw", DEFAULT);
pub const HOME_LATEST: Op = Op::get("HomeLatestTimeline", "0dateTVgvXjpkf7kyBZy0g", DEFAULT);
pub const BOOKMARKS: Op = Op::get("Bookmarks", "XD0ViOeSOW4YoeNTGjVaYw", DEFAULT);
pub const BOOKMARK_FOLDERS: Op = Op::get("BookmarkFoldersSlice", "i78YDd0Tza-dV4SYs58kRg", DEFAULT);
pub const BOOKMARK_FOLDER: Op =
  Op::get("BookmarkFolderTimeline", "hNY7X2xE2N7HVF6Qb_mu6w", DEFAULT);
pub const LIST_TWEETS: Op = Op::get(
  "ListLatestTweetsTimeline",
  "FVWmROVvhgjRPC-4jAUh8A",
  DEFAULT,
);
pub const LIST_OWNERSHIPS: Op = Op::get("ListOwnerships", "5eUATiy7RZHeOMVM5ZZIcg", DEFAULT);
pub const EXPLORE_TIMELINE: Op = Op::get("GenericTimelineById", "S_hzVUv1trgZ_5ruDe2IoA", DEFAULT);
pub const NOTIFICATIONS: Op = Op::get("NotificationsTimeline", "gzC0OYBCnfdYS4M4Gue7BA", DEFAULT);
// Lazy chunk `shared~bundle.QuoteTweetActivity~bundle.TweetActivity` (the "post engagements" screen).
pub const FAVORITERS: Op = Op::get("Favoriters", "HaFAhly6sDpoeqGEbFb2Ig", DEFAULT);
pub const RETWEETERS: Op = Op::get("Retweeters", "UBCF0EF800cPqREAeu1uuA", DEFAULT);

// ── analytics: Relay queries of x.com/i/account_analytics (`bundle.AccountAnalytics`) ──

/// Rollup of a few metrics shown to every account (the analytics upsell).
pub const FREE_ROLLUP: Op = Op::get("useFetchAnalyticsQuery", "5JkoDLRvQrXv2QV4U5gKFg", NONE);
/// Account metrics per day (`organic_metrics_time_series`).
pub const ACCOUNT_SERIES: Op = Op::get("overviewDataUserQuery", "NlJ6RM-hgHxt-iu9cPQz7A", NONE);
/// Post metrics per day.
pub const POST_SERIES: Op = Op::get("overviewDataPostQuery", "9c83mWUXFc4RuVLInF9SOQ", NONE);
/// Post metrics since publication (`organic_metrics_total`).
pub const POST_TOTALS: Op = Op::get(
  "postDetailsProviderMetricsTotalQuery",
  "yLIUkOUqs-4MT8I5gUyztQ",
  NONE,
);
/// Audience of the account: engagements by age, gender, app, network, country, hour.
pub const ACCOUNT_AUDIENCE: Op =
  Op::get("audienceOverviewDataQuery", "H47r_cVD9Uu-qMQLktBCKA", NONE);
pub const POST_AUDIENCE: Op = Op::get(
  "postDetailsProviderAudienceQuery",
  "S4-UXaX7xV7kLLelCMej3g",
  NONE,
);
/// The account's posts of a period with their metrics (the "Content" tab).
pub const CONTENT: Op = Op::get(
  "ContentPageV2UserTweetsQuery",
  "7uyOLS6aSCF-HaYHhaZXhw",
  NONE,
);

// ── writing ───────────────────────────────────────────────────────────

pub const CREATE_TWEET: Op = Op::post("CreateTweet", "WNkbkQ_JLIofjdukTXahVA", CREATE);
pub const DELETE_TWEET: Op = Op::post("DeleteTweet", "nxpZCY2K-I6QoFHAHeojFQ", NONE);
pub const FAVORITE: Op = Op::post("FavoriteTweet", "lI07N6Otwv1PhnEgXILM7A", NONE);
pub const UNFAVORITE: Op = Op::post("UnfavoriteTweet", "ZYKSe-w7KEslx3JhSIk5LA", NONE);
pub const RETWEET: Op = Op::post("CreateRetweet", "mbRO74GrOvSfRcJnlMapnQ", NONE);
pub const UNRETWEET: Op = Op::post("DeleteRetweet", "ZyZigVsNiFO6v1dEks1eWg", NONE);
pub const BOOKMARK: Op = Op::post("CreateBookmark", "aoDbu3RHznuiSkQ9aNM67Q", NONE);
pub const UNBOOKMARK: Op = Op::post("DeleteBookmark", "Wlmlj2-xzyS1GN3a6cj-mQ", NONE);
