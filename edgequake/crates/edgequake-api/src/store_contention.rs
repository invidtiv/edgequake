//! Store contention SLOs for queue-metrics /ready (SPEC-057 P3).
//!
//! Mirrors [`crate::task_queue_pressure`]: assessor owns thresholds; handlers
//! only project DTOs. Signals are real pool utilization + process-local
//! compensation quarantine totals — no invented latency histograms.

use edgequake_storage::compensation_quarantine_total;

/// Store contention pressure level for dashboards and readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreContentionLevel {
    Normal,
    Elevated,
    Critical,
}

impl StoreContentionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Critical => "critical",
        }
    }
}

/// Snapshot of store contention vs configured thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreContentionSnapshot {
    pub level: StoreContentionLevel,
    pub db_pool_utilization: Option<f64>,
    pub db_pool_util_warn: f64,
    pub db_pool_util_critical: f64,
    pub compensation_quarantine_total: u64,
    pub compensation_quarantine_warn: u64,
    pub compensation_quarantine_critical: u64,
    pub operator_action: Option<String>,
}

/// Pool utilization warn threshold (`EDGEQUAKE_DB_POOL_UTIL_WARN`, default 0.75).
pub fn db_pool_util_warn_threshold() -> f64 {
    std::env::var("EDGEQUAKE_DB_POOL_UTIL_WARN")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.75_f64)
        .clamp(0.0_f64, 1.0_f64)
}

/// Pool utilization critical threshold (`EDGEQUAKE_DB_POOL_UTIL_CRITICAL`, default 0.90).
pub fn db_pool_util_critical_threshold() -> f64 {
    std::env::var("EDGEQUAKE_DB_POOL_UTIL_CRITICAL")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.90_f64)
        .clamp(0.0_f64, 1.0_f64)
        .max(db_pool_util_warn_threshold())
}

/// Quarantine count warn (`EDGEQUAKE_COMPENSATION_QUARANTINE_WARN`, default 1).
pub fn compensation_quarantine_warn_threshold() -> u64 {
    std::env::var("EDGEQUAKE_COMPENSATION_QUARANTINE_WARN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// Quarantine count critical (`EDGEQUAKE_COMPENSATION_QUARANTINE_CRITICAL`, default 5).
pub fn compensation_quarantine_critical_threshold() -> u64 {
    std::env::var("EDGEQUAKE_COMPENSATION_QUARANTINE_CRITICAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
        .max(compensation_quarantine_warn_threshold().saturating_add(1))
}

/// Compute pool utilization from sqlx pool size/idle (active / size).
pub fn pool_utilization(size: u32, idle: u32) -> Option<f64> {
    if size == 0 {
        return None;
    }
    let active = size.saturating_sub(idle);
    Some(active as f64 / size as f64)
}

/// Per-role pool utilization snapshot (SPEC-090 F-090-28).
#[derive(Debug, Clone, PartialEq)]
pub struct RolePoolUtilization {
    pub role: &'static str,
    pub size: u32,
    pub idle: u32,
    pub utilization: Option<f64>,
}

/// Max utilization across role pools (readiness uses the hottest role).
pub fn max_role_pool_utilization(roles: &[RolePoolUtilization]) -> Option<f64> {
    roles.iter().filter_map(|r| r.utilization).reduce(f64::max)
}

/// Build role util rows from a [`edgequake_storage::PgPoolBundle`].
#[cfg(feature = "postgres")]
pub fn role_utils_from_bundle(
    bundle: &edgequake_storage::PgPoolBundle,
) -> Vec<RolePoolUtilization> {
    [
        ("query", &bundle.query),
        ("ingest", &bundle.ingest),
        ("queue", &bundle.queue),
        ("admin", &bundle.admin),
    ]
    .into_iter()
    .map(|(role, pool)| {
        let size = pool.size();
        let idle = pool.num_idle().min(u32::MAX as usize) as u32;
        RolePoolUtilization {
            role,
            size,
            idle,
            utilization: pool_utilization(size, idle),
        }
    })
    .collect()
}

/// Assess store contention from pool util + quarantine totals.
pub fn assess_store_contention(db_pool_utilization: Option<f64>) -> StoreContentionSnapshot {
    let util_warn = db_pool_util_warn_threshold();
    let util_critical = db_pool_util_critical_threshold();
    let q_warn = compensation_quarantine_warn_threshold();
    let q_critical = compensation_quarantine_critical_threshold();
    let quarantine = compensation_quarantine_total();

    let util_level = match db_pool_utilization {
        Some(u) if u >= util_critical => StoreContentionLevel::Critical,
        Some(u) if u >= util_warn => StoreContentionLevel::Elevated,
        _ => StoreContentionLevel::Normal,
    };
    let q_level = if quarantine >= q_critical {
        StoreContentionLevel::Critical
    } else if quarantine >= q_warn {
        StoreContentionLevel::Elevated
    } else {
        StoreContentionLevel::Normal
    };

    let level = match (util_level, q_level) {
        (StoreContentionLevel::Critical, _) | (_, StoreContentionLevel::Critical) => {
            StoreContentionLevel::Critical
        }
        (StoreContentionLevel::Elevated, _) | (_, StoreContentionLevel::Elevated) => {
            StoreContentionLevel::Elevated
        }
        _ => StoreContentionLevel::Normal,
    };

    let operator_action = match level {
        StoreContentionLevel::Critical => Some(format!(
            "Store contention critical (pool_util={:?}, quarantine={quarantine}). \
             Scale DB pool / reduce ingest concurrency; inspect compensation_quarantine:* KV DLQ \
             and Prometheus edgequake_compensation_quarantine_total",
            db_pool_utilization
        )),
        StoreContentionLevel::Elevated => Some(format!(
            "Store contention elevated (pool_util={:?}, quarantine={quarantine}). \
             Watch queue-metrics.store_contention and compensation quarantine DLQ",
            db_pool_utilization
        )),
        StoreContentionLevel::Normal => None,
    };

    StoreContentionSnapshot {
        level,
        db_pool_utilization,
        db_pool_util_warn: util_warn,
        db_pool_util_critical: util_critical,
        compensation_quarantine_total: quarantine,
        compensation_quarantine_warn: q_warn,
        compensation_quarantine_critical: q_critical,
        operator_action,
    }
}

/// True when `/ready` should block traffic due to store contention.
pub fn readiness_blocked_by_store(db_pool_utilization: Option<f64>) -> bool {
    assess_store_contention(db_pool_utilization).level == StoreContentionLevel::Critical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_utilization_math() {
        assert_eq!(pool_utilization(10, 2), Some(0.8));
        assert_eq!(pool_utilization(0, 0), None);
    }

    #[test]
    fn assess_normal_when_empty() {
        // Quarantine may be non-zero if other tests ran; only assert shape fields.
        let snap = assess_store_contention(Some(0.1));
        assert!(snap.db_pool_util_warn > 0.0);
        assert!(snap.db_pool_util_critical >= snap.db_pool_util_warn);
    }

    #[test]
    fn readiness_blocked_when_pool_util_critical() {
        let prev_warn = std::env::var("EDGEQUAKE_DB_POOL_UTIL_WARN").ok();
        let prev_crit = std::env::var("EDGEQUAKE_DB_POOL_UTIL_CRITICAL").ok();
        std::env::set_var("EDGEQUAKE_DB_POOL_UTIL_WARN", "0.50");
        std::env::set_var("EDGEQUAKE_DB_POOL_UTIL_CRITICAL", "0.80");
        assert!(readiness_blocked_by_store(Some(0.95)));
        assert!(!readiness_blocked_by_store(Some(0.10)));
        match prev_warn {
            Some(v) => std::env::set_var("EDGEQUAKE_DB_POOL_UTIL_WARN", v),
            None => std::env::remove_var("EDGEQUAKE_DB_POOL_UTIL_WARN"),
        }
        match prev_crit {
            Some(v) => std::env::set_var("EDGEQUAKE_DB_POOL_UTIL_CRITICAL", v),
            None => std::env::remove_var("EDGEQUAKE_DB_POOL_UTIL_CRITICAL"),
        }
    }
}
