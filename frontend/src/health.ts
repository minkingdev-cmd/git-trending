import type { HealthStatus } from "./types";

/** Chinese labels for health badges (display only; trust API `health`). */
export const HEALTH_LABELS: Record<HealthStatus, string> = {
  active: "活跃",
  stale: "陈旧",
  archived: "已归档",
  unknown: "未知",
};

export function normalizeHealth(raw: string | null | undefined): HealthStatus {
  if (raw === "active" || raw === "stale" || raw === "archived" || raw === "unknown") {
    return raw;
  }
  return "unknown";
}

export function healthLabel(raw: string | null | undefined): string {
  return HEALTH_LABELS[normalizeHealth(raw)];
}

/** Relative time in Chinese (days/months/years), or absolute date. */
export function formatRelativeTime(
  iso: string | null | undefined,
  now: Date = new Date(),
): string {
  if (!iso) return "未知";
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "未知";
  const diffMs = now.getTime() - t;
  if (diffMs < 0) return formatAbsoluteDate(iso);
  const days = Math.floor(diffMs / (24 * 60 * 60 * 1000));
  if (days < 1) {
    const hours = Math.floor(diffMs / (60 * 60 * 1000));
    if (hours < 1) return "刚刚";
    return `${hours} 小时前`;
  }
  if (days < 30) return `${days} 天前`;
  if (days < 365) {
    const months = Math.floor(days / 30);
    return `${months} 个月前`;
  }
  const years = Math.floor(days / 365);
  return `${years} 年前`;
}

export function formatAbsoluteDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = iso.slice(0, 10);
  if (/^\d{4}-\d{2}-\d{2}$/.test(d)) return d;
  try {
    return new Date(iso).toISOString().slice(0, 10);
  } catch {
    return iso;
  }
}

export function formatAge(
  createdAtGh: string | null | undefined,
  now: Date = new Date(),
): string {
  if (!createdAtGh) return "—";
  const t = Date.parse(createdAtGh);
  if (!Number.isFinite(t)) return "—";
  const days = Math.floor((now.getTime() - t) / (24 * 60 * 60 * 1000));
  if (days < 0) return formatAbsoluteDate(createdAtGh);
  if (days < 30) return `${days} 天`;
  if (days < 365) {
    const months = Math.floor(days / 30);
    return `${months} 个月`;
  }
  const years = Math.floor(days / 365);
  const remMonths = Math.floor((days % 365) / 30);
  if (remMonths > 0) return `${years} 年 ${remMonths} 个月`;
  return `${years} 年`;
}

export interface HealthDetailInput {
  health?: string | null;
  pushed_at?: string | null;
  open_issues_count?: number | null;
  latest_release_at?: string | null;
  created_at_gh?: string | null;
  archived?: boolean;
}

/** Multi-line tooltip text for HealthBadge. */
export function healthTooltip(input: HealthDetailInput, now: Date = new Date()): string {
  const status = normalizeHealth(input.health);
  const pushRel = formatRelativeTime(input.pushed_at, now);
  const pushAbs = input.pushed_at ? formatAbsoluteDate(input.pushed_at) : "未知";
  const issues =
    input.open_issues_count == null ? "—" : String(input.open_issues_count);
  const release = input.latest_release_at
    ? `${formatRelativeTime(input.latest_release_at, now)} (${formatAbsoluteDate(input.latest_release_at)})`
    : "无 release / 未知";
  const age = formatAge(input.created_at_gh, now);
  const lines = [
    `健康：${HEALTH_LABELS[status]}`,
    `Last push：${pushRel}${input.pushed_at ? ` · ${pushAbs}` : ""}`,
    `Open issues：${issues}（含 PR）`,
    `Latest release：${release}`,
    `Age：${age}`,
  ];
  return lines.join("\n");
}
