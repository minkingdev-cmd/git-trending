import {
  healthLabel,
  healthTooltip,
  normalizeHealth,
  type HealthDetailInput,
} from "../health";
import type { HealthStatus } from "../types";

export interface HealthBadgeProps extends HealthDetailInput {
  className?: string;
}

const VARIANT: Record<HealthStatus, string> = {
  active: "health-badge active",
  stale: "health-badge stale",
  archived: "health-badge archived",
  unknown: "health-badge unknown",
};

/**
 * Small health pill: active=green, stale=amber, archived=gray, unknown=muted.
 * Details (push / issues / release / age) via title tooltip.
 */
export default function HealthBadge({
  health,
  pushed_at,
  open_issues_count,
  latest_release_at,
  created_at_gh,
  archived,
  className,
}: HealthBadgeProps) {
  const status = normalizeHealth(health);
  const title = healthTooltip({
    health: status,
    pushed_at,
    open_issues_count,
    latest_release_at,
    created_at_gh,
    archived,
  });
  const cls = [VARIANT[status], className].filter(Boolean).join(" ");

  return (
    <span className={cls} title={title} tabIndex={0} role="status">
      {healthLabel(status)}
    </span>
  );
}
