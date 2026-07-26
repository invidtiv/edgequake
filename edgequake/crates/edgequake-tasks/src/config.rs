//! Task storage / queue configuration from environment (SPEC-090 Wave 3).

/// Default max workers for queue utilization metrics.
pub const DEFAULT_TASK_MAX_WORKERS: u32 = 4;

/// Default retention for terminal tasks (days).
pub const DEFAULT_TASK_RETENTION_DAYS: u32 = 30;

/// Bounded sample size for claim_next fair workspace pick (SPEC-090 F-090-11).
pub const CLAIM_SAMPLE_LIMIT: i64 = 1000;

/// Env var for max worker count in queue metrics (`EDGEQUAKE_TASK_MAX_WORKERS`).
pub const TASK_MAX_WORKERS_ENV: &str = "EDGEQUAKE_TASK_MAX_WORKERS";

/// Env var for terminal task retention days (`EDGEQUAKE_TASK_RETENTION_DAYS`).
pub const TASK_RETENTION_DAYS_ENV: &str = "EDGEQUAKE_TASK_RETENTION_DAYS";

/// Resolve max workers from `EDGEQUAKE_TASK_MAX_WORKERS` (default 4, min 1).
pub fn task_max_workers_from_env() -> u32 {
    std::env::var(TASK_MAX_WORKERS_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_TASK_MAX_WORKERS)
}

/// Resolve retention days from `EDGEQUAKE_TASK_RETENTION_DAYS` (default 30, min 1).
pub fn task_retention_days_from_env() -> u32 {
    std::env::var(TASK_RETENTION_DAYS_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_TASK_RETENTION_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_env() {
        assert_eq!(task_max_workers_from_env(), DEFAULT_TASK_MAX_WORKERS);
        assert_eq!(task_retention_days_from_env(), DEFAULT_TASK_RETENTION_DAYS);
    }
}
