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
}

export interface LanguageOption {
  language: string;
  count: number;
}
