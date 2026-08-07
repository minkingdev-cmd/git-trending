export interface LanguageShare {
  name: string;
  pct: number;
  bytes?: number;
}

export interface LeaderboardItem {
  rank: number;
  full_name: string;
  html_url: string;
  description: string | null;
  /** Deprecated primary language; prefer `languages`. */
  language: string | null;
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

export interface LeaderboardResponse {
  date: string;
  board: string;
  language: string | null;
  languages_filter?: string[];
  q?: string | null;
  topics_filter?: string[];
  topic_mode?: string;
  items: LeaderboardItem[];
  topic_facets?: TopicFacet[];
  language_facets?: LanguageFacet[];
}

export interface MeResponse {
  user_id: number;
  username: string;
  is_admin: boolean;
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
