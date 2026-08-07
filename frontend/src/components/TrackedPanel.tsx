import { compact } from "../api";
import type { TrackedRepoItem, TrackedStatus } from "../types";

interface Props {
  items: TrackedRepoItem[];
  loading: boolean;
  /** Load/list error — replaces list content when set. */
  error: string | null;
  /** Untrack action error — banner only; list stays visible. */
  untrackError?: string | null;
  /** True when q/topics/languages filters are active (empty-state copy). */
  hasFilters?: boolean;
  onUntrack: (fullName: string) => void;
  onSelectRepo: (fullName: string) => void;
  onRetry?: () => void;
  untracking?: string | null;
}

function statusLabel(status: string): string {
  if (status === "on_board") return "已在榜";
  if (status === "pending") return "待快照";
  return "跟踪中";
}

function statusBadgeClass(status: string): string {
  if (status === "pending") return "badge-track pending";
  if (status === "tracking") return "badge-track user-only";
  return "badge-track";
}

function statusHint(status: string): string {
  if (status === "on_board") return "也在今日总榜/趋势中";
  if (status === "pending") return "等待下次 collector 快照";
  return "每日独立快照";
}

function formatAddedAt(iso: string): string {
  if (!iso) return "—";
  // Prefer YYYY-MM-DD for compact display; fall back to raw.
  const d = iso.slice(0, 10);
  if (/^\d{4}-\d{2}-\d{2}$/.test(d)) return d;
  try {
    return new Date(iso).toISOString().slice(0, 10);
  } catch {
    return iso;
  }
}

export function normalizeTrackedStatus(status: string): TrackedStatus {
  if (status === "on_board" || status === "pending" || status === "tracking") {
    return status;
  }
  return "tracking";
}

export default function TrackedPanel({
  items,
  loading,
  error,
  untrackError = null,
  hasFilters = false,
  onUntrack,
  onSelectRepo,
  onRetry,
  untracking,
}: Props) {
  return (
    <div className="tracked-panel show" aria-label="我的跟踪仓库">
      <h3>我的跟踪仓库</h3>
      <p className="tp-sub">
        手动添加的仓库会进入每日快照队列，即使未进趋势/总榜 top100 也可查看指标与历史。
        已在榜内的条目会标「已在榜」。
      </p>

      {loading && (
        <div className="space-y-2 py-4" aria-busy="true" aria-label="加载中">
          {[1, 2, 3].map((i) => (
            <div key={i} className="skeleton-row" style={{ opacity: 1 - i * 0.15 }} />
          ))}
        </div>
      )}

      {!loading && error && (
        <div className="tracked-empty">
          <p style={{ color: "var(--danger)" }}>{error}</p>
          {onRetry && (
            <button
              type="button"
              className="btn"
              style={{ marginTop: 8 }}
              onClick={onRetry}
            >
              重试
            </button>
          )}
        </div>
      )}

      {/* Untrack failure: banner only — keep list visible. */}
      {!loading && !error && untrackError && (
        <p
          className="field-error"
          role="alert"
          style={{ marginBottom: 12 }}
        >
          {untrackError}
        </p>
      )}

      {!loading && !error && items.length === 0 && (
        <div className="tracked-empty">
          {hasFilters
            ? "无匹配的跟踪仓库。试试清除筛选或换关键词。"
            : "还没有跟踪仓库。点击右上角「＋ 添加仓库」开始。"}
        </div>
      )}

      {!loading && !error && items.length > 0 && (
        <div className="tracked-list">
          {items.map((item) => {
            const st = normalizeTrackedStatus(item.status);
            const busy = untracking === item.full_name;
            return (
              <div className="tracked-row" key={item.full_name} data-repo={item.full_name}>
                <div className="tr-name">
                  <a
                    href={item.html_url || `https://github.com/${item.full_name}`}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {item.full_name}
                  </a>
                  <span className={statusBadgeClass(st)}>{statusLabel(st)}</span>
                  {item.description && (
                    <div
                      style={{
                        fontSize: 12,
                        color: "var(--text-3)",
                        fontWeight: 400,
                        marginTop: 2,
                      }}
                    >
                      {item.description}
                    </div>
                  )}
                </div>
                <div className="tr-meta">
                  {compact(item.stars)} ★ · {compact(item.forks)} ⑂
                  {item.license ? ` · ${item.license}` : ""}
                  {" · "}添加于 {formatAddedAt(item.added_at)}
                </div>
                <div className="tr-status">{statusHint(st)}</div>
                <div className="tr-actions">
                  <button
                    type="button"
                    className="btn"
                    onClick={() => onSelectRepo(item.full_name)}
                    title="查看历史趋势"
                  >
                    趋势
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => onUntrack(item.full_name)}
                    disabled={busy}
                    title="取消跟踪"
                  >
                    {busy ? "取消中…" : "取消跟踪"}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
