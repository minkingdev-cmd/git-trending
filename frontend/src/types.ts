export interface LanguageShare {
  name: string;
  pct: number;
  bytes?: number;
}

/** Runtime health from API: active | stale | archived | unknown. */
export type HealthStatus = "active" | "stale" | "archived" | "unknown";

/** Shared health fields on leaderboard / tracked rows. */
export interface RepoHealthFields {
  pushed_at?: string | null;
  archived?: boolean;
  open_issues_count?: number | null;
  created_at_gh?: string | null;
  latest_release_at?: string | null;
  health?: HealthStatus | string;
}

export interface LeaderboardItem extends RepoHealthFields {
  rank: number;
  full_name: string;
  html_url: string;
  description: string | null;
  /** Deprecated primary language; prefer `languages`. */
  language: string | null;
  /** SPDX id or short key (MIT, Apache-2.0, …). */
  license: string | null;
  topics: string[];
  languages: LanguageShare[];
  stars: number;
  forks: number;
  watchers: number | null;
  stars_today: number | null;
  tracked_by_me: boolean;
}

export interface TopicFacet {
  topic: string;
  count: number;
}

export interface LanguageFacet {
  language: string;
  count: number;
}

export interface LicenseFacet {
  license: string;
  count: number;
}

export interface LeaderboardResponse {
  date: string;
  board: string;
  language: string | null;
  languages_filter?: string[];
  licenses_filter?: string[];
  q?: string | null;
  topics_filter?: string[];
  topic_mode?: string;
  items: LeaderboardItem[];
  topic_facets?: TopicFacet[];
  language_facets?: LanguageFacet[];
  license_facets?: LicenseFacet[];
}

export interface MeResponse {
  user_id: number;
  username: string;
  is_admin: boolean;
  /** Whether the user has saved a personal GitHub PAT (never returns the token). */
  has_github_token?: boolean;
}

/** Token path used for a discover search. */
export type DiscoverAuthMode = "user" | "shared";

export interface DiscoverItem extends RepoHealthFields {
  full_name: string;
  html_url: string;
  description: string | null;
  language: string | null;
  license: string | null;
  stars: number;
  forks: number;
  topics: string[];
  already_tracked: boolean;
  in_local_index: boolean;
}

export interface DiscoverSearchResponse {
  items: DiscoverItem[];
  page: number;
  per_page: number;
  total_count: number;
  incomplete_results: boolean;
  auth_mode: DiscoverAuthMode;
}

export interface GithubTokenStatus {
  has_github_token: boolean;
}

export interface LanguageOption {
  language: string;
  count: number;
}

export interface MetaResponse {
  date: string | null;
  boards: Record<string, number>;
  dates?: string[];
}

export interface HistoryPoint {
  date: string;
  stars: number;
  forks: number;
  watchers: number | null;
  stars_today: number | null;
}

export interface HistoryResponse {
  full_name: string;
  board: string;
  from: string;
  to: string;
  points: HistoryPoint[];
}

export interface InviteItem {
  code: string;
  max_uses: number;
  used_count: number;
  revoked: boolean;
}

export interface UserItem {
  id: number;
  username: string;
  is_admin: boolean;
  created_at: string;
}

/** Status of a user-tracked repo relative to public boards / snapshots. */
export type TrackedStatus = "on_board" | "tracking" | "pending";

export interface TrackedRepoItem extends RepoHealthFields {
  full_name: string;
  html_url: string;
  description: string | null;
  license?: string | null;
  languages: LanguageShare[];
  topics: string[];
  stars: number;
  forks: number;
  watchers: number | null;
  status: TrackedStatus | string;
  added_at: string;
}

export interface TrackedListResponse {
  items: TrackedRepoItem[];
}

export interface LookupResponse {
  full_name: string;
  html_url: string;
  description: string | null;
  license?: string | null;
  languages: LanguageShare[];
  topics: string[];
  stars: number;
  forks: number;
  watchers: number | null;
  on_leaderboard: boolean;
  already_tracked: boolean;
}

export interface TrackResponse {
  item: TrackedRepoItem;
}

export interface RepoRefBody {
  full_name?: string;
  url?: string;
}
