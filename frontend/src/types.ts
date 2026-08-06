export interface LeaderboardItem {
  rank: number;
  full_name: string;
  html_url: string;
  description: string | null;
  language: string | null;
  stars: number;
  forks: number;
  watchers: number | null;
  stars_today: number | null;
}

export interface LeaderboardResponse {
  date: string;
  board: string;
  language: string | null;
  items: LeaderboardItem[];
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
