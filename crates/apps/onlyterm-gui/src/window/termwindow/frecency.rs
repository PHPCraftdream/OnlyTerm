use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Tracks how recently and frequently an item was accessed.
#[serde_with::serde_as]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Frecency {
    /// The score decays to half its value after this duration.
    #[serde_as(as = "serde_with::DurationSeconds<i64>")]
    half_life: Duration,
    #[serde_as(as = "serde_with::TimestampSeconds<i64>")]
    last_accessed: DateTime<Utc>,
    frecency: f64,
    num_accesses: u64,
}

impl Default for Frecency {
    fn default() -> Self {
        Self::new()
    }
}

impl Frecency {
    /// Creates a new score with no recorded accesses.
    pub fn new() -> Self {
        Self::new_at_time(Utc::now())
    }

    /// Creates a new score with no recorded accesses at `now`.
    pub fn new_at_time(now: DateTime<Utc>) -> Self {
        Self {
            half_life: Duration::days(3),
            frecency: 0.0,
            last_accessed: now,
            num_accesses: 0,
        }
    }

    /// Records an access at the current time.
    pub fn register_access(&mut self) {
        self.register_access_at_time(Utc::now());
    }

    /// Records an access at `now`.
    pub fn register_access_at_time(&mut self, now: DateTime<Utc>) {
        let prior = self.score_at_time(now);
        self.last_accessed = now;
        self.set_frecency_at_time(1.0 + prior, now);
        self.num_accesses += 1;
    }

    /// Computes the score at the current time.
    pub fn score(&self) -> f64 {
        self.score_at_time(Utc::now())
    }

    /// Computes the score at `now`.
    pub fn score_at_time(&self, now: DateTime<Utc>) -> f64 {
        let elapsed = duration_secs_f64(now - self.last_accessed);
        self.frecency / 2.0_f64.powf(elapsed / duration_secs_f64(self.half_life))
    }

    fn set_frecency_at_time(&mut self, value: f64, now: DateTime<Utc>) {
        let elapsed = duration_secs_f64(now - self.last_accessed);
        self.frecency = value * 2.0_f64.powf(elapsed / duration_secs_f64(self.half_life));
    }
}

fn duration_secs_f64(dur: Duration) -> f64 {
    dur.num_milliseconds() as f64 / 1000.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64) -> bool {
        a == b || (a - b).abs() <= f64::EPSILON
    }

    fn assert_approx_eq(a: f64, b: f64) {
        assert!(approx_eq(a, b), "expected {} to be approx. {}", a, b);
    }

    #[test]
    fn score_decays_and_accesses_accumulate() {
        let now = Utc::now();
        let mut f = Frecency::new_at_time(now);
        assert_eq!(f.score_at_time(now), 0.);
        f.register_access_at_time(now);
        assert_eq!(f.score_at_time(now), 1.0);

        assert_approx_eq(f.score_at_time(now + Duration::days(1)), 0.7937005259840997);
        assert_approx_eq(f.score_at_time(now + Duration::days(3)), 0.5);

        f.register_access_at_time(now + Duration::days(3));
        assert_approx_eq(f.score_at_time(now + Duration::days(3)), 1.5);
        assert_approx_eq(f.score_at_time(now + Duration::days(30)), 0.0029296875);
        assert_approx_eq(
            f.score_at_time(now + Duration::days(300)),
            0.0000000000000000000000000000023665827156630354,
        );
        assert_eq!(f.num_accesses, 2);
    }

    #[test]
    fn serialize_new_score_uses_legacy_shape() {
        use chrono::TimeZone;

        let now = Utc.with_ymd_and_hms(2022, 8, 31, 22, 16, 0).unwrap();
        let f = Frecency::new_at_time(now);
        assert_eq!(
            serde_json::to_string(&f).unwrap(),
            r#"{"half_life":259200,"last_accessed":1661984160,"frecency":0.0,"num_accesses":0}"#
        );
    }

    #[test]
    fn loads_persisted_history_and_preserves_its_score() {
        use chrono::TimeZone;

        // Legacy serialized shape, with non-zero history.
        let legacy =
            r#"{"half_life":259200,"last_accessed":1661984160,"frecency":1.5,"num_accesses":7}"#;
        let f: Frecency = serde_json::from_str(legacy).unwrap();
        let last_accessed = Utc.timestamp_opt(1_661_984_160, 0).unwrap();
        let now = last_accessed + Duration::days(3);

        assert_eq!(f.last_accessed, last_accessed);
        assert_eq!(f.num_accesses, 7);
        assert_eq!(f.score_at_time(now), 0.75);
        assert_eq!(serde_json::to_string(&f).unwrap(), legacy);
    }
}
