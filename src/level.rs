pub const MIN_LEVEL: u8 = 1;
pub const MAX_LEVEL: u8 = 5;

#[derive(Clone, Copy, Debug)]
pub struct LevelProfile {
    pub own_elo: u16,
    pub temperature: f32,
    pub candidate_count: usize,
    pub max_depth: usize,
    pub policy_weight: i32,
    pub sample_policy: bool,
    pub max_score_drop: i32,
}

pub fn profile(level: u8) -> LevelProfile {
    match level.clamp(MIN_LEVEL, MAX_LEVEL) {
        1 => LevelProfile {
            own_elo: 800,
            temperature: 1.0,
            candidate_count: 6,
            max_depth: 2,
            policy_weight: 0,
            sample_policy: true,
            max_score_drop: 250,
        },
        2 => LevelProfile {
            own_elo: 1_100,
            temperature: 0.8,
            candidate_count: 8,
            max_depth: 2,
            policy_weight: 0,
            sample_policy: true,
            max_score_drop: 150,
        },
        3 => LevelProfile {
            own_elo: 1_400,
            temperature: 0.9,
            candidate_count: 8,
            max_depth: 4,
            policy_weight: 25,
            sample_policy: false,
            max_score_drop: 0,
        },
        4 => LevelProfile {
            own_elo: 1_700,
            temperature: 0.7,
            candidate_count: 8,
            max_depth: 6,
            policy_weight: 20,
            sample_policy: false,
            max_score_drop: 0,
        },
        _ => LevelProfile {
            own_elo: 2_100,
            temperature: 0.5,
            candidate_count: usize::MAX,
            max_depth: 128,
            policy_weight: 0,
            sample_policy: false,
            max_score_drop: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_LEVEL, MIN_LEVEL, profile};

    #[test]
    fn levels_are_monotonic() {
        let mut previous_elo = 0;
        for level in MIN_LEVEL..=MAX_LEVEL {
            let current = profile(level);
            assert!(current.own_elo > previous_elo);
            assert!(current.temperature > 0.0);
            previous_elo = current.own_elo;
        }
    }

    #[test]
    fn low_levels_sample_and_high_levels_search() {
        assert!(profile(1).sample_policy);
        assert!(profile(2).sample_policy);
        assert_eq!(profile(1).max_depth, 2);
        assert_eq!(profile(2).max_depth, 2);
        assert!(profile(1).max_score_drop > 0);
        assert!(profile(2).max_score_drop > 0);
        assert!(!profile(3).sample_policy);
        assert!(!profile(MAX_LEVEL).sample_policy);
        assert!(profile(3).max_depth < profile(4).max_depth);
        assert!(profile(4).max_depth < profile(MAX_LEVEL).max_depth);
    }

    #[test]
    fn out_of_range_levels_are_clamped() {
        assert_eq!(profile(0).own_elo, profile(MIN_LEVEL).own_elo);
        assert_eq!(profile(255).own_elo, profile(MAX_LEVEL).own_elo);
    }
}
