import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError,
  compact,
  discoverSearch,
  trackRepo,
  UnauthorizedError,
  type ApiErrorBody,
} from "../api";
import type { DiscoverAuthMode, DiscoverItem, TrackedRepoItem } from "../types";
import type { DiscoverSort } from "../urlState";
import { hasDiscoverCondition } from "../urlState";
import HealthBadge from "./HealthBadge";

const ACTIVE_WITHIN_DAYS = 90;
const PER_PAGE = 30;
const MAX_PAGE = 10;
const DEBOUNCE_MS = 300;

export interface DiscoverFilters {
  dq: string;
  dlanguage: string;
  dlicense: string;
  dminStars: number | null;
  dexcludeArchived: boolean;
  dactiveWithin: number | null;
  dsort: DiscoverSort;
  dpage: number;
}

interface Props {
  filters: DiscoverFilters;
  onFiltersChange: (patch: Partial<DiscoverFilters>) => void;
  hasGithubToken: boolean;
  onOpenTokenModal: () => void;
  onTracked: (item: TrackedRepoItem) => void;
  onUnauthorized?: () => void;
  showDesc: boolean;
}

function rateLimitMessage(body: ApiErrorBody | undefined, fallback: string): string {
  const scope = body?.scope;
  const secs = body?.retry_after_secs;
  const wait =
    typeof secs === "number" && secs > 0 ? `请约 ${secs} 秒后再试。` : "请稍后再试。";
  if (body?.error === "github_rate_limited") {
    return `GitHub 搜索配额已用尽。${wait}`;
  }
  if (scope === "global") {
    return `共享搜索配额繁忙（全站限流）。${wait} 配置个人 Token 可走独立配额。`;
  }
  if (scope === "user") {
    return `请求过于频繁。${wait}`;
  }
  return fallback || `请求过于频繁。${wait}`;
}

function errorMessageFromApi(e: ApiError): string {
  const body = e.body as ApiErrorBody | undefined;
  if (e.status === 429) {
    return rateLimitMessage(body, e.message);
  }
  if (e.status === 503 || body?.error === "github_token_required") {
    return "需要 GitHub Token：请配置个人 Token，或由管理员配置服务端 GITHUB_TOKEN。";
  }
  if (body?.error === "github_auth_failed") {
    return "GitHub 认证失败：个人 Token 可能已失效，请更新 Token。";
  }
  if (e.status === 400) {
    return e.message || "搜索条件无效";
  }
  return e.message || `搜索失败（HTTP ${e.status}）`;
}

export default function DiscoverPanel({
  filters,
  onFiltersChange,
  hasGithubToken,
  onOpenTokenModal,
  onTracked,
  onUnauthorized,
  showDesc,
}: Props) {
  const [qDraft, setQDraft] = useState(filters.dq);
  const [langDraft, setLangDraft] = useState(filters.dlanguage);
  const [licenseDraft, setLicenseDraft] = useState(filters.dlicense);
  const [minStarsDraft, setMinStarsDraft] = useState(
    filters.dminStars != null ? String(filters.dminStars) : "",
  );

  const [items, setItems] = useState<DiscoverItem[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [incomplete, setIncomplete] = useState(false);
  const [authMode, setAuthMode] = useState<DiscoverAuthMode | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryAfterSecs, setRetryAfterSecs] = useState(0);
  const [tracking, setTracking] = useState<string | null>(null);
  const [trackError, setTrackError] = useState<string | null>(null);

  const abortRef = useRef(0);
  const retryTimerRef = useRef<number | null>(null);
  const retryAfterRef = useRef(0);

  // Sync drafts when filters change externally (e.g. URL).
  useEffect(() => {
    setQDraft(filters.dq);
  }, [filters.dq]);
  useEffect(() => {
    setLangDraft(filters.dlanguage);
  }, [filters.dlanguage]);
  useEffect(() => {
    setLicenseDraft(filters.dlicense);
  }, [filters.dlicense]);
  useEffect(() => {
    setMinStarsDraft(
      filters.dminStars != null ? String(filters.dminStars) : "",
    );
  }, [filters.dminStars]);

  // Debounce keyword → filters.dq
  useEffect(() => {
    if (qDraft === filters.dq) return;
    const t = window.setTimeout(() => {
      onFiltersChange({ dq: qDraft, dpage: 1 });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [qDraft, filters.dq, onFiltersChange]);

  // Debounce language / license text fields
  useEffect(() => {
    if (langDraft === filters.dlanguage) return;
    const t = window.setTimeout(() => {
      onFiltersChange({ dlanguage: langDraft, dpage: 1 });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [langDraft, filters.dlanguage, onFiltersChange]);

  useEffect(() => {
    if (licenseDraft === filters.dlicense) return;
    const t = window.setTimeout(() => {
      onFiltersChange({ dlicense: licenseDraft, dpage: 1 });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [licenseDraft, filters.dlicense, onFiltersChange]);

  // min_stars: commit on blur / enter via applyMinStars
  function applyMinStars() {
    const raw = minStarsDraft.trim();
    if (!raw) {
      if (filters.dminStars != null) {
        onFiltersChange({ dminStars: null, dpage: 1 });
      }
      return;
    }
    const n = Number.parseInt(raw, 10);
    if (!Number.isFinite(n) || n < 0) {
      setMinStarsDraft(
        filters.dminStars != null ? String(filters.dminStars) : "",
      );
      return;
    }
    if (n !== filters.dminStars) {
      onFiltersChange({ dminStars: n, dpage: 1 });
    }
  }

  // Countdown for 429
  useEffect(() => {
    retryAfterRef.current = retryAfterSecs;
    if (retryAfterSecs <= 0) return;
    retryTimerRef.current = window.setTimeout(() => {
      setRetryAfterSecs((s) => {
        const next = Math.max(0, s - 1);
        retryAfterRef.current = next;
        return next;
      });
    }, 1000);
    return () => {
      if (retryTimerRef.current != null) {
        window.clearTimeout(retryTimerRef.current);
      }
    };
  }, [retryAfterSecs]);

  const canSearch = hasDiscoverCondition(filters);
  const searchBlocked = retryAfterSecs > 0;

  const load = useCallback(async () => {
    if (!hasDiscoverCondition(filters)) {
      setItems([]);
      setTotalCount(0);
      setIncomplete(false);
      setAuthMode(null);
      setError(null);
      setLoading(false);
      return;
    }
    if (retryAfterRef.current > 0) return;

    const seq = ++abortRef.current;
    setLoading(true);
    setError(null);
    setTrackError(null);
    try {
      const res = await discoverSearch({
        q: filters.dq,
        language: filters.dlanguage,
        license: filters.dlicense,
        minStars: filters.dminStars,
        excludeArchived: filters.dexcludeArchived,
        activeWithin: filters.dactiveWithin,
        sort: filters.dsort,
        page: filters.dpage,
      });
      if (seq !== abortRef.current) return;
      setItems(res.items ?? []);
      setTotalCount(res.total_count ?? 0);
      setIncomplete(!!res.incomplete_results);
      setAuthMode(res.auth_mode ?? null);
    } catch (e) {
      if (seq !== abortRef.current) return;
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        const body = e.body as ApiErrorBody | undefined;
        if (e.status === 429) {
          const secs =
            typeof body?.retry_after_secs === "number" && body.retry_after_secs > 0
              ? body.retry_after_secs
              : 12;
          retryAfterRef.current = secs;
          setRetryAfterSecs(secs);
        }
        setError(errorMessageFromApi(e));
        setItems([]);
        return;
      }
      setError(e instanceof Error ? e.message : "搜索失败");
      setItems([]);
    } finally {
      if (seq === abortRef.current) setLoading(false);
    }
  }, [filters, onUnauthorized]);
  useEffect(() => {
    void load();
  }, [load]);

  async function onTrack(fullName: string) {
    setTracking(fullName);
    setTrackError(null);
    try {
      const item = await trackRepo({ full_name: fullName });
      setItems((cur) =>
        cur.map((r) =>
          r.full_name.toLowerCase() === fullName.toLowerCase()
            ? { ...r, already_tracked: true }
            : r,
        ),
      );
      onTracked(item);
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        if (e.status === 409) {
          setTrackError("已达跟踪上限（最多 50 个仓库）");
        } else {
          setTrackError(e.message || `加入跟踪失败（HTTP ${e.status}）`);
        }
        return;
      }
      setTrackError(e instanceof Error ? e.message : "加入跟踪失败");
    } finally {
      setTracking(null);
    }
  }

  const maxPage = Math.min(
    MAX_PAGE,
    Math.max(1, Math.ceil(totalCount / PER_PAGE) || 1),
  );
  const page = Math.min(filters.dpage, maxPage);

  return (
    <div className="discover-panel" aria-label="发现 — 搜索 GitHub">
      <section className="controls-panel" aria-label="发现筛选">
        <div className="controls-row">
          <div className="search-wrap" style={{ flex: 1, minWidth: 180 }}>
            <span className="search-icon" aria-hidden="true">
              ⌕
            </span>
            <input
              className="field-input"
              type="search"
              value={qDraft}
              onChange={(e) => setQDraft(e.target.value)}
              placeholder="搜索 GitHub：关键词…"
              autoComplete="off"
              spellCheck={false}
              aria-label="发现关键词"
              disabled={searchBlocked}
            />
          </div>
          <input
            className="field-input"
            type="text"
            value={langDraft}
            onChange={(e) => setLangDraft(e.target.value)}
            placeholder="语言（如 Rust）"
            aria-label="语言"
            style={{ width: 120 }}
            disabled={searchBlocked}
          />
          <input
            className="field-input"
            type="text"
            value={licenseDraft}
            onChange={(e) => setLicenseDraft(e.target.value)}
            placeholder="license（mit）"
            aria-label="License"
            style={{ width: 120 }}
            disabled={searchBlocked}
          />
          <input
            className="field-input"
            type="number"
            min={0}
            value={minStarsDraft}
            onChange={(e) => setMinStarsDraft(e.target.value)}
            onBlur={() => applyMinStars()}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                applyMinStars();
              }
            }}
            placeholder="最少 ★"
            aria-label="最少 stars"
            style={{ width: 96 }}
            disabled={searchBlocked}
          />
          <select
            className="field-select"
            value={filters.dsort}
            onChange={(e) =>
              onFiltersChange({
                dsort: e.target.value === "updated" ? "updated" : "stars",
                dpage: 1,
              })
            }
            aria-label="排序"
            disabled={searchBlocked}
          >
            <option value="stars">按 stars</option>
            <option value="updated">按更新</option>
          </select>
        </div>

        <div className="controls-row">
          <label
            className="toggle-chip"
            title="默认排除已归档仓库"
          >
            <input
              type="checkbox"
              checked={filters.dexcludeArchived}
              onChange={(e) =>
                onFiltersChange({
                  dexcludeArchived: e.target.checked,
                  dpage: 1,
                })
              }
              disabled={searchBlocked}
            />
            排除已归档
          </label>
          <label
            className="toggle-chip"
            title={`仅显示最近 ${ACTIVE_WITHIN_DAYS} 天内有 push 的仓库`}
          >
            <input
              type="checkbox"
              checked={filters.dactiveWithin === ACTIVE_WITHIN_DAYS}
              onChange={(e) =>
                onFiltersChange({
                  dactiveWithin: e.target.checked ? ACTIVE_WITHIN_DAYS : null,
                  dpage: 1,
                })
              }
              disabled={searchBlocked}
            />
            仅活跃({ACTIVE_WITHIN_DAYS}天)
          </label>
          <button
            type="button"
            className="header-btn"
            onClick={onOpenTokenModal}
            title="配置个人 GitHub Token 以提升配额"
          >
            {hasGithubToken ? "Token ✓" : "GitHub Token"}
          </button>
          {authMode && (
            <span
              className="badge-track"
              title={
                authMode === "user"
                  ? "本次搜索使用个人 PAT 配额"
                  : "本次搜索使用站点共享 GITHUB_TOKEN"
              }
              style={{ marginLeft: 0 }}
            >
              {authMode === "user" ? "个人配额" : "共享配额"}
            </span>
          )}
          {!hasGithubToken && (
            <span className="text-sm" style={{ color: "var(--text-3)" }}>
              配置个人 Token 可减少与他人抢共享桶
            </span>
          )}
        </div>
      </section>

      <div className="result-meta" style={{ marginTop: 12 }}>
        <div className="active-filters">
          {!canSearch && (
            <span style={{ color: "var(--muted)" }}>
              请输入关键词，或设置语言 / license / 最少 stars / 仅活跃
            </span>
          )}
          {canSearch && !loading && !error && (
            <span>
              {totalCount.toLocaleString()} 个结果（GitHub）
              {incomplete ? " · 结果可能不完整" : ""}
              {` · 第 ${page}/${maxPage} 页`}
            </span>
          )}
        </div>
      </div>

      {trackError && (
        <p className="field-error" role="alert" style={{ marginBottom: 8 }}>
          {trackError}
        </p>
      )}

      {loading && (
        <div className="space-y-2 py-4" aria-busy="true" aria-label="搜索中">
          {[1, 2, 3, 4, 5].map((i) => (
            <div
              key={i}
              className="skeleton-row"
              style={{ opacity: 1 - i * 0.12 }}
            />
          ))}
        </div>
      )}

      {!loading && error && (
        <div className="space-y-2 py-8 text-center">
          <p style={{ color: "var(--danger)" }}>{error}</p>
          {(error.includes("Token") || error.includes("token")) && (
            <button
              type="button"
              className="btn btn-primary"
              onClick={onOpenTokenModal}
              style={{ marginTop: 8 }}
            >
              配置 GitHub Token
            </button>
          )}
          <div>
            <button
              type="button"
              onClick={() => {
                setRetryAfterSecs(0);
                void load();
              }}
              className="text-sm hover:underline"
              style={{ color: "var(--link)" }}
              disabled={searchBlocked}
            >
              {searchBlocked ? `重试（${retryAfterSecs}s）` : "重试"}
            </button>
          </div>
        </div>
      )}

      {!loading && !error && canSearch && items.length === 0 && (
        <p className="py-8 text-center" style={{ color: "var(--muted)" }}>
          无匹配仓库。试试放宽条件。
        </p>
      )}

      {!loading && !error && items.length > 0 && (
        <div className="table-wrap">
          <table className="lb">
            <thead>
              <tr>
                <th style={{ width: 44 }}>#</th>
                <th>Repo</th>
                <th style={{ width: 100 }}>语言</th>
                <th style={{ width: 90 }}>License</th>
                <th className="num" style={{ width: 80 }}>
                  ★
                </th>
                <th style={{ width: 88 }}>健康</th>
                <th style={{ width: 120 }}>操作</th>
              </tr>
            </thead>
            <tbody>
              {items.map((item, idx) => {
                const rank = (page - 1) * PER_PAGE + idx + 1;
                const busy = tracking === item.full_name;
                return (
                  <tr key={item.full_name}>
                    <td className="rank">{rank}</td>
                    <td>
                      <div className="repo-cell">
                        <a
                          href={
                            item.html_url ||
                            `https://github.com/${item.full_name}`
                          }
                          target="_blank"
                          rel="noopener noreferrer"
                          className="repo-name"
                        >
                          {item.full_name}
                        </a>
                        {item.in_local_index && (
                          <span
                            className="badge-track user-only"
                            title="已在本地索引"
                          >
                            本地
                          </span>
                        )}
                        {item.already_tracked && (
                          <span className="badge-track" title="已在你的跟踪列表">
                            已跟踪
                          </span>
                        )}
                        {showDesc && item.description && (
                          <div className="repo-desc">{item.description}</div>
                        )}
                      </div>
                    </td>
                    <td>{item.language || "—"}</td>
                    <td>{item.license || "—"}</td>
                    <td className="num">{compact(item.stars)}</td>
                    <td>
                      <HealthBadge
                        health={item.health}
                        pushed_at={item.pushed_at}
                        open_issues_count={item.open_issues_count}
                        latest_release_at={item.latest_release_at}
                        created_at_gh={item.created_at_gh}
                        archived={item.archived}
                      />
                    </td>
                    <td>
                      <button
                        type="button"
                        className="btn btn-primary"
                        style={{ padding: "4px 10px", fontSize: 12 }}
                        disabled={item.already_tracked || busy || searchBlocked}
                        onClick={() => void onTrack(item.full_name)}
                      >
                        {item.already_tracked
                          ? "已跟踪"
                          : busy
                            ? "加入中…"
                            : "加入跟踪"}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {!loading && !error && canSearch && totalCount > 0 && (
        <div
          className="controls-row"
          style={{ marginTop: 12, justifyContent: "center", gap: 12 }}
        >
          <button
            type="button"
            className="btn"
            disabled={page <= 1 || searchBlocked}
            onClick={() => onFiltersChange({ dpage: page - 1 })}
          >
            上一页
          </button>
          <span className="text-sm" style={{ color: "var(--text-3)" }}>
            {page} / {maxPage}
            {totalCount > MAX_PAGE * PER_PAGE
              ? `（GitHub 共约 ${totalCount.toLocaleString()}，最多 ${MAX_PAGE} 页）`
              : ""}
          </span>
          <button
            type="button"
            className="btn"
            disabled={page >= maxPage || searchBlocked}
            onClick={() => onFiltersChange({ dpage: page + 1 })}
          >
            下一页
          </button>
        </div>
      )}

      <p
        className="text-sm"
        style={{ color: "var(--muted)", marginTop: 12, lineHeight: 1.5 }}
      >
        发现结果来自 GitHub Search，不写入榜单。加入跟踪后进入「我的跟踪」并走
        enrich。total_count 可能被 GitHub 封顶；每页 {PER_PAGE} 条，最多{" "}
        {MAX_PAGE} 页。
      </p>
    </div>
  );
}
