use chrono::{DateTime, Duration, Utc};

pub const STALE_AFTER_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Active,
    Stale,
    Archived,
    Unknown,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthStatus::Active => "active",
            HealthStatus::Stale => "stale",
            HealthStatus::Archived => "archived",
            HealthStatus::Unknown => "unknown",
        }
    }
}

pub fn compute_health(
    archived: bool,
    pushed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> HealthStatus {
    if archived {
        return HealthStatus::Archived;
    }
    let Some(pushed) = pushed_at else {
        return HealthStatus::Unknown;
    };
    if now.signed_duration_since(pushed) > Duration::days(STALE_AFTER_DAYS) {
        HealthStatus::Stale
    } else {
        HealthStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn archived_wins() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 0, 0, 0).unwrap();
        let push = now - Duration::days(1);
        assert_eq!(compute_health(true, Some(push), now), HealthStatus::Archived);
    }

    #[test]
    fn null_push_unknown() {
        let now = Utc::now();
        assert_eq!(compute_health(false, None, now), HealthStatus::Unknown);
    }

    #[test]
    fn exactly_90_days_is_active() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap();
        let push = now - Duration::days(90);
        assert_eq!(compute_health(false, Some(push), now), HealthStatus::Active);
    }

    #[test]
    fn over_90_days_stale() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap();
        let push = now - Duration::days(90) - Duration::seconds(1);
        assert_eq!(compute_health(false, Some(push), now), HealthStatus::Stale);
    }
}
