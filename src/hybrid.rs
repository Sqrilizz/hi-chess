use crate::features::{Policy, top_policy_moves};
use crate::level::LevelProfile;
use chess::ChessMove;

#[derive(Clone, Copy, Debug)]
pub struct HybridConfig<'a> {
    pub policy: Option<&'a Policy>,
    pub level_profile: LevelProfile,
    pub random_value: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ScoredMove {
    pub chess_move: ChessMove,
    pub score: i32,
}

pub fn filter_candidates(
    moves: Vec<ChessMove>,
    policy: Option<&Policy>,
    candidate_count: usize,
) -> Vec<ChessMove> {
    let Some(policy) = policy else {
        return moves;
    };
    let candidates = top_policy_moves(policy, candidate_count);
    let mut filtered: Vec<ChessMove> = moves
        .into_iter()
        .filter(|chess_move| {
            candidates
                .iter()
                .any(|candidate| candidate.chess_move == *chess_move)
        })
        .collect();
    filtered.sort_unstable_by(|left, right| {
        policy_probability(policy, *right)
            .partial_cmp(&policy_probability(policy, *left))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.cmp(right))
    });
    filtered
}

fn policy_probability(policy: &Policy, chess_move: ChessMove) -> f32 {
    policy
        .moves
        .iter()
        .find(|policy_move| policy_move.chess_move == chess_move)
        .map(|policy_move| policy_move.probability)
        .unwrap_or(0.0)
}

pub fn select_move(
    scores: &[ScoredMove],
    policy: Option<&Policy>,
    policy_weight: i32,
) -> Option<ScoredMove> {
    scores.iter().copied().max_by(|left, right| {
        let left_score = adjusted_score(*left, policy, policy_weight);
        let right_score = adjusted_score(*right, policy, policy_weight);
        left_score
            .total_cmp(&right_score)
            .then_with(|| left.chess_move.cmp(&right.chess_move))
    })
}

fn adjusted_score(scored: ScoredMove, policy: Option<&Policy>, policy_weight: i32) -> f32 {
    if policy_weight == 0 {
        return scored.score as f32;
    }
    let Some(policy) = policy else {
        return scored.score as f32;
    };
    let probability = policy
        .moves
        .iter()
        .find(|policy_move| policy_move.chess_move == scored.chess_move)
        .map(|policy_move| policy_move.probability)
        .unwrap_or(0.0);
    scored.score as f32 + policy_weight as f32 * probability.max(1e-6).ln()
}

#[cfg(test)]
mod tests {
    use super::{ScoredMove, filter_candidates, select_move};
    use crate::features::{POLICY_MOVES, move_index, policy_from_logits};
    use chess::{Board, ChessMove, Square};

    fn policy() -> crate::features::Policy {
        let board = Board::default();
        let mut logits = [0.0; POLICY_MOVES];
        logits[move_index(&board, ChessMove::new(Square::E2, Square::E4, None))] = 8.0;
        logits[move_index(&board, ChessMove::new(Square::D2, Square::D4, None))] = 6.0;
        policy_from_logits(&board, &logits, [0.0; 3], 1.0)
    }

    #[test]
    fn filters_to_policy_candidates() {
        let board = Board::default();
        let moves = chess::MoveGen::new_legal(&board).collect();
        let filtered = filter_candidates(moves, Some(&policy()), 2);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0], ChessMove::new(Square::E2, Square::E4, None));
        assert!(filtered.contains(&ChessMove::new(Square::E2, Square::E4, None)));
        assert!(filtered.contains(&ChessMove::new(Square::D2, Square::D4, None)));
    }

    #[test]
    fn policy_weight_can_change_selection() {
        let scores = [
            ScoredMove {
                chess_move: ChessMove::new(Square::E2, Square::E4, None),
                score: 10,
            },
            ScoredMove {
                chess_move: ChessMove::new(Square::D2, Square::D4, None),
                score: 30,
            },
        ];
        assert_eq!(
            select_move(&scores, None, 0).unwrap().chess_move.get_dest(),
            Square::D4
        );
        assert_eq!(
            select_move(&scores, Some(&policy()), 100)
                .unwrap()
                .chess_move
                .get_dest(),
            Square::E4
        );
    }
}
