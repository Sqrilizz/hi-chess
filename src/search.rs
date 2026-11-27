use crate::{
    evaluation::evaluate,
    features::sample_policy,
    hybrid::{HybridConfig, ScoredMove, filter_candidates, select_move},
    level::profile,
    move_order::{is_capture, is_tactical, order_moves},
    transposition::{Bound, TranspositionTable},
};
use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const MATE_SCORE: i32 = 30_000;
pub const MATE_THRESHOLD: i32 = MATE_SCORE - 128;
const INFINITE_SCORE: i32 = 32_000;
const MAX_PLY: usize = 128;
const MAX_QUIESCENCE_PLY: usize = 8;
const STOP_CHECK_INTERVAL: u64 = 1_024;
const HALFMOVE_DRAW_LIMIT: u16 = 100;
const DEFAULT_TT_ENTRIES: usize = 1 << 18;

#[derive(Clone, Debug, Default)]
pub struct SearchLimits {
    pub depth: Option<u16>,
    pub nodes: Option<u64>,
    pub movetime: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct SearchInfo {
    pub depth: usize,
    pub selective_depth: usize,
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub best_move: ChessMove,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<ChessMove>,
    pub score: i32,
    pub depth: usize,
    pub selective_depth: usize,
    pub nodes: u64,
    pub elapsed: Duration,
    pub root_moves: Vec<ScoredMove>,
}

struct RootSearch {
    move_scores: Vec<ScoredMove>,
    completed: bool,
}

#[derive(Clone, Copy)]
struct NodeState {
    ply: usize,
    halfmove_clock: u16,
}

pub struct Search {
    limits: SearchLimits,
    stop: Arc<AtomicBool>,
    started: Instant,
    deadline: Option<Instant>,
    nodes: u64,
    selective_depth: usize,
    killers: Vec<[Option<ChessMove>; 2]>,
    history: [[i32; 64]; 64],
    transpositions: TranspositionTable,
    stopped: bool,
}

impl Search {
    pub fn new(limits: SearchLimits, stop: Arc<AtomicBool>) -> Self {
        let started = Instant::now();
        let deadline = limits.movetime.map(|duration| started + duration);

        Self {
            limits,
            stop,
            started,
            deadline,
            nodes: 0,
            selective_depth: 0,
            killers: vec![[None, None]; MAX_PLY + 1],
            history: [[0; 64]; 64],
            transpositions: TranspositionTable::new(DEFAULT_TT_ENTRIES),
            stopped: false,
        }
    }

    pub fn search<F>(
        &mut self,
        board: Board,
        prior_hashes: &[u64],
        halfmove_clock: u16,
        on_info: F,
    ) -> SearchResult
    where
        F: FnMut(SearchInfo),
    {
        self.search_with_policy(
            board,
            prior_hashes,
            halfmove_clock,
            HybridConfig {
                policy: None,
                level_profile: profile(5),
                random_value: 0.0,
            },
            on_info,
        )
    }

    pub fn search_with_policy<F>(
        &mut self,
        board: Board,
        prior_hashes: &[u64],
        halfmove_clock: u16,
        config: HybridConfig<'_>,
        mut on_info: F,
    ) -> SearchResult
    where
        F: FnMut(SearchInfo),
    {
        let root_moves: Vec<ChessMove> = MoveGen::new_legal(&board).collect();
        let Some(first_move) = root_moves.first().copied() else {
            return SearchResult {
                best_move: None,
                score: 0,
                depth: 0,
                selective_depth: 0,
                nodes: 0,
                elapsed: self.elapsed(),
                root_moves: Vec::new(),
            };
        };

        let policy = config.policy;
        let level_profile = if policy.is_some() {
            config.level_profile
        } else {
            profile(5)
        };

        if level_profile.sample_policy
            && let Some(policy) = policy
            && let Some(best_move) = sample_policy(policy, config.random_value)
        {
            let score = evaluate(&board);
            let elapsed = self.elapsed();
            on_info(SearchInfo {
                depth: 0,
                selective_depth: 0,
                score,
                nodes: 0,
                elapsed,
                best_move,
            });
            return SearchResult {
                best_move: Some(best_move),
                score,
                depth: 0,
                selective_depth: 0,
                nodes: 0,
                elapsed,
                root_moves: Vec::new(),
            };
        }

        let max_depth = self
            .limits
            .depth
            .map(usize::from)
            .unwrap_or(MAX_PLY)
            .clamp(1, MAX_PLY)
            .min(level_profile.max_depth);
        let mut path = prior_hashes.to_vec();
        let mut result = SearchResult {
            best_move: Some(first_move),
            score: evaluate(&board),
            depth: 0,
            selective_depth: 0,
            nodes: 0,
            elapsed: Duration::ZERO,
            root_moves: Vec::new(),
        };

        for depth in 1..=max_depth {
            if self.should_stop() {
                break;
            }

            self.stopped = false;
            let root = self.search_root(
                &board,
                depth,
                halfmove_clock,
                &mut path,
                result.best_move,
                HybridConfig {
                    policy,
                    level_profile,
                    random_value: config.random_value,
                },
            );

            if let Some(selected) =
                select_move(&root.move_scores, policy, level_profile.policy_weight)
            {
                if root.completed {
                    result = SearchResult {
                        best_move: Some(selected.chess_move),
                        score: selected.score,
                        depth,
                        selective_depth: self.selective_depth,
                        nodes: self.nodes,
                        elapsed: self.elapsed(),
                        root_moves: root.move_scores.clone(),
                    };
                    on_info(SearchInfo {
                        depth,
                        selective_depth: self.selective_depth,
                        score: selected.score,
                        nodes: self.nodes,
                        elapsed: self.elapsed(),
                        best_move: selected.chess_move,
                    });
                } else if result.depth == 0 {
                    result.best_move = Some(selected.chess_move);
                    result.score = selected.score;
                    result.root_moves = root.move_scores;
                }
            }

            if self.stopped || is_mate_score(result.score) {
                break;
            }
        }

        result.nodes = self.nodes;
        result.selective_depth = self.selective_depth;
        result.elapsed = self.elapsed();
        result
    }

    fn search_root(
        &mut self,
        board: &Board,
        depth: usize,
        halfmove_clock: u16,
        path: &mut Vec<u64>,
        preferred: Option<ChessMove>,
        config: HybridConfig<'_>,
    ) -> RootSearch {
        let moves = order_moves(board, preferred, 0, &self.killers, &self.history);
        let moves = filter_candidates(moves, config.policy, config.level_profile.candidate_count);
        let mut alpha = -INFINITE_SCORE;
        let mut move_scores = Vec::with_capacity(moves.len());

        for chess_move in moves {
            if self.should_stop() {
                break;
            }

            let next_board = board.make_move_new(chess_move);
            let next_halfmove = halfmove_clock_after(board, halfmove_clock, chess_move);
            path.push(board.get_hash());
            let score = -self.negamax(
                next_board,
                depth - 1,
                -INFINITE_SCORE,
                -alpha,
                NodeState {
                    ply: 1,
                    halfmove_clock: next_halfmove,
                },
                path,
            );
            path.pop();

            move_scores.push(ScoredMove { chess_move, score });
            alpha = alpha.max(score);
        }

        RootSearch {
            move_scores,
            completed: !self.stopped,
        }
    }

    fn negamax(
        &mut self,
        board: Board,
        depth: usize,
        mut alpha: i32,
        beta: i32,
        node: NodeState,
        path: &mut Vec<u64>,
    ) -> i32 {
        let ply = node.ply;
        let halfmove_clock = node.halfmove_clock;
        self.nodes += 1;
        self.selective_depth = self.selective_depth.max(ply);

        if self.should_stop() {
            return 0;
        }

        match board.status() {
            BoardStatus::Checkmate => return -MATE_SCORE + ply as i32,
            BoardStatus::Stalemate => return 0,
            BoardStatus::Ongoing => {}
        }

        if ply > 0 && is_draw(&board, halfmove_clock, path) {
            return 0;
        }

        let hash = board.get_hash();
        let mut preferred = None;
        if depth > 0
            && let Some(entry) = self.transpositions.probe(hash)
        {
            let score = score_from_table(entry.score, ply);
            if entry.depth as usize >= depth && bound_cutoff(entry.bound, score, alpha, beta) {
                return score;
            }
            preferred = entry.best_move;
        }

        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply, halfmove_clock, path);
        }

        let original_alpha = alpha;
        let moves = order_moves(&board, preferred, ply, &self.killers, &self.history);
        let mut best_score = -INFINITE_SCORE;
        let mut best_move = None;
        let mut completed = true;

        for (move_index, chess_move) in moves.into_iter().enumerate() {
            if self.should_stop() {
                completed = false;
                break;
            }

            let next_board = board.make_move_new(chess_move);
            let next_halfmove = halfmove_clock_after(&board, halfmove_clock, chess_move);
            path.push(board.get_hash());
            let (child_alpha, child_beta) = if move_index == 0 {
                (-beta, -alpha)
            } else {
                (-alpha - 1, -alpha)
            };
            let mut score = -self.negamax(
                next_board,
                depth - 1,
                child_alpha,
                child_beta,
                NodeState {
                    ply: ply + 1,
                    halfmove_clock: next_halfmove,
                },
                path,
            );
            if move_index > 0 && score > alpha && score < beta {
                score = -self.negamax(
                    next_board,
                    depth - 1,
                    -beta,
                    -alpha,
                    NodeState {
                        ply: ply + 1,
                        halfmove_clock: next_halfmove,
                    },
                    path,
                );
            }
            path.pop();
            if self.stopped {
                completed = false;
                break;
            }

            if score > best_score {
                best_score = score;
                best_move = Some(chess_move);
            }
            alpha = alpha.max(score);

            if alpha >= beta {
                if !is_capture(&board, chess_move) {
                    self.store_killer(chess_move, ply);
                    let source = chess_move.get_source().to_index();
                    let destination = chess_move.get_dest().to_index();
                    self.history[source][destination] += (depth * depth) as i32;
                }
                break;
            }
        }

        if completed {
            let bound = if best_score >= beta {
                Bound::Lower
            } else if best_score <= original_alpha {
                Bound::Upper
            } else {
                Bound::Exact
            };
            self.transpositions.store(
                hash,
                score_to_table(best_score, ply),
                depth,
                bound,
                best_move,
            );
        }

        best_score
    }

    fn quiescence(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        halfmove_clock: u16,
        path: &mut Vec<u64>,
    ) -> i32 {
        self.nodes += 1;
        self.selective_depth = self.selective_depth.max(ply);

        if self.should_stop() {
            return 0;
        }

        let in_check = board.checkers().popcnt() > 0;
        if in_check && board.status() == BoardStatus::Checkmate {
            return -MATE_SCORE + ply as i32;
        }

        if ply > 0 && is_draw(&board, halfmove_clock, path) {
            return 0;
        }

        let stand_pat = if in_check { 0 } else { evaluate(&board) };
        if !in_check {
            if stand_pat >= beta {
                return stand_pat;
            }
            alpha = alpha.max(stand_pat);
        }

        if ply >= MAX_QUIESCENCE_PLY {
            return evaluate(&board);
        }

        let mut moves: Vec<ChessMove> = MoveGen::new_legal(&board)
            .filter(|chess_move| in_check || is_tactical(&board, *chess_move))
            .collect();
        let scored_moves: Vec<(i32, ChessMove)> = moves
            .drain(..)
            .map(|chess_move| {
                let score = if is_capture(&board, chess_move) {
                    capture_order_score(&board, chess_move)
                } else {
                    chess_move
                        .get_promotion()
                        .map(crate::evaluation::piece_value)
                        .unwrap_or(0)
                };
                (score, chess_move)
            })
            .collect();
        let mut scored_moves = scored_moves;
        scored_moves.sort_unstable_by(|left, right| {
            right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1))
        });

        if scored_moves.is_empty() {
            return if in_check {
                evaluate(&board)
            } else {
                stand_pat
            };
        }

        let mut best_score = if in_check { -INFINITE_SCORE } else { stand_pat };
        for (_, chess_move) in scored_moves {
            if self.should_stop() {
                break;
            }

            let next_board = board.make_move_new(chess_move);
            let next_halfmove = halfmove_clock_after(&board, halfmove_clock, chess_move);
            path.push(board.get_hash());
            let score = -self.quiescence(next_board, -beta, -alpha, ply + 1, next_halfmove, path);
            path.pop();

            if score > best_score {
                best_score = score;
            }
            alpha = alpha.max(score);

            if alpha >= beta {
                break;
            }
        }

        best_score
    }

    fn store_killer(&mut self, chess_move: ChessMove, ply: usize) {
        if ply >= self.killers.len() {
            return;
        }

        if self.killers[ply][0] != Some(chess_move) {
            self.killers[ply][1] = self.killers[ply][0];
            self.killers[ply][0] = Some(chess_move);
        }
    }

    fn should_stop(&mut self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            self.stopped = true;
            return true;
        }

        if self.limits.nodes.is_some_and(|limit| self.nodes >= limit) {
            self.stopped = true;
            return true;
        }

        if self.nodes.is_multiple_of(STOP_CHECK_INTERVAL)
            && self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.stopped = true;
            return true;
        }

        false
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

pub fn halfmove_clock_after(board: &Board, halfmove_clock: u16, chess_move: ChessMove) -> u16 {
    let moving_piece = board.piece_on(chess_move.get_source());
    let resets_clock =
        moving_piece == Some(Piece::Pawn) || board.piece_on(chess_move.get_dest()).is_some();

    if resets_clock {
        0
    } else {
        halfmove_clock.saturating_add(1)
    }
}

fn is_draw(board: &Board, halfmove_clock: u16, path: &[u64]) -> bool {
    halfmove_clock >= HALFMOVE_DRAW_LIMIT
        || path.contains(&board.get_hash())
        || insufficient_material(board)
}

pub fn insufficient_material(board: &Board) -> bool {
    let major_pieces =
        *board.pieces(Piece::Pawn) | *board.pieces(Piece::Rook) | *board.pieces(Piece::Queen);
    if major_pieces.popcnt() != 0 {
        return false;
    }

    let white_minors = *board.pieces(Piece::Knight) & *board.color_combined(Color::White);
    let white_minors = white_minors.popcnt()
        + (*board.pieces(Piece::Bishop) & *board.color_combined(Color::White)).popcnt();
    let black_minors = *board.pieces(Piece::Knight) & *board.color_combined(Color::Black);
    let black_minors = black_minors.popcnt()
        + (*board.pieces(Piece::Bishop) & *board.color_combined(Color::Black)).popcnt();

    white_minors <= 1 && black_minors <= 1
}

fn capture_order_score(board: &Board, chess_move: ChessMove) -> i32 {
    let victim = board
        .piece_on(chess_move.get_dest())
        .map(crate::evaluation::piece_value)
        .unwrap_or(crate::evaluation::piece_value(Piece::Pawn));
    let attacker = board
        .piece_on(chess_move.get_source())
        .map(crate::evaluation::piece_value)
        .unwrap_or(0);
    victim * 16 - attacker
}

fn is_mate_score(score: i32) -> bool {
    score.abs() > MATE_THRESHOLD
}

fn bound_cutoff(bound: Bound, score: i32, alpha: i32, beta: i32) -> bool {
    match bound {
        Bound::Exact => true,
        Bound::Lower => score >= beta,
        Bound::Upper => score <= alpha,
    }
}

fn score_to_table(score: i32, ply: usize) -> i32 {
    if score > MATE_THRESHOLD {
        score + ply as i32
    } else if score < -MATE_THRESHOLD {
        score - ply as i32
    } else {
        score
    }
}

fn score_from_table(score: i32, ply: usize) -> i32 {
    if score > MATE_THRESHOLD {
        score - ply as i32
    } else if score < -MATE_THRESHOLD {
        score + ply as i32
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Bound, MATE_SCORE, Search, SearchLimits, bound_cutoff, halfmove_clock_after,
        score_from_table, score_to_table,
    };
    use crate::{
        features::{POLICY_MOVES, move_index, policy_from_logits},
        hybrid::HybridConfig,
        level::profile,
    };
    use chess::{Board, ChessMove, MoveGen, Piece, Square};
    use std::{str::FromStr, sync::Arc, time::Duration};

    fn atomic_stop() -> Arc<std::sync::atomic::AtomicBool> {
        Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    #[test]
    fn mate_scores_survive_table_roundtrip() {
        let score = MATE_SCORE - 3;
        assert_eq!(score_from_table(score_to_table(score, 5), 5), score);
        assert_eq!(score_from_table(score_to_table(-score, 5), 5), -score);
    }

    #[test]
    fn bounds_cutoff_only_valid_windows() {
        assert!(bound_cutoff(Bound::Exact, 0, -10, 10));
        assert!(bound_cutoff(Bound::Lower, 10, -10, 10));
        assert!(!bound_cutoff(Bound::Lower, 0, -10, 10));
        assert!(bound_cutoff(Bound::Upper, -10, -10, 10));
        assert!(!bound_cutoff(Bound::Upper, 0, -10, 10));
    }

    #[test]
    fn finds_mate_in_one() {
        let board = Board::from_str("7k/8/5KQ1/8/8/8/8/8 w - - 0 1").unwrap();
        let mut search = Search::new(
            SearchLimits {
                depth: Some(2),
                ..SearchLimits::default()
            },
            atomic_stop(),
        );
        let result = search.search(board, &[], 0, |_| {});
        assert_eq!(
            result.best_move,
            Some(ChessMove::new(Square::G6, Square::G7, None))
        );
        assert_eq!(result.score, MATE_SCORE - 1);
    }

    #[test]
    fn returns_only_legal_moves() {
        let board = Board::default();
        let mut search = Search::new(
            SearchLimits {
                depth: Some(2),
                ..SearchLimits::default()
            },
            atomic_stop(),
        );
        let result = search.search(board, &[], 0, |_| {});
        assert!(
            result
                .best_move
                .is_some_and(|chess_move| board.legal(chess_move))
        );
    }

    #[test]
    fn node_limit_is_respected() {
        let board = Board::default();
        let mut search = Search::new(
            SearchLimits {
                depth: Some(20),
                nodes: Some(100),
                ..SearchLimits::default()
            },
            atomic_stop(),
        );
        let result = search.search(board, &[], 0, |_| {});
        assert!(result.nodes <= 100);
        assert!(result.best_move.is_some());
    }

    #[test]
    fn halfmove_clock_resets_on_pawn_move_and_capture() {
        let board = Board::default();
        let pawn_move = ChessMove::new(Square::E2, Square::E4, None);
        assert_eq!(halfmove_clock_after(&board, 0, pawn_move), 0);

        let board = Board::from_str("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let capture = ChessMove::new(Square::E4, Square::D5, None);
        assert_eq!(halfmove_clock_after(&board, 42, capture), 0);

        let quiet = ChessMove::new(Square::E1, Square::D1, None);
        assert_eq!(halfmove_clock_after(&board, 42, quiet), 43);
    }

    #[test]
    fn terminal_position_has_no_best_move() {
        let board = Board::from_str("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        let mut search = Search::new(SearchLimits::default(), atomic_stop());
        let result = search.search(board, &[], 0, |_| {});
        assert_eq!(result.best_move, None);
        assert_eq!(MoveGen::new_legal(&board).count(), 0);
        assert_eq!(board.piece_on(Square::H8), Some(Piece::King));
    }

    #[test]
    fn movetime_produces_a_result() {
        let board = Board::default();
        let mut search = Search::new(
            SearchLimits {
                movetime: Some(Duration::from_millis(5)),
                ..SearchLimits::default()
            },
            atomic_stop(),
        );
        let result = search.search(board, &[], 0, |_| {});
        assert!(result.best_move.is_some());
        assert!(result.elapsed < Duration::from_secs(2));
    }

    #[test]
    fn low_level_samples_from_policy() {
        let board = Board::default();
        let mut logits = [0.0; POLICY_MOVES];
        logits[move_index(ChessMove::new(Square::E2, Square::E4, None))] = 20.0;
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        let mut search = Search::new(SearchLimits::default(), atomic_stop());
        let result = search.search_with_policy(
            board,
            &[],
            0,
            HybridConfig {
                policy: Some(&policy),
                level_profile: profile(1),
                random_value: 0.5,
            },
            |_| {},
        );
        assert_eq!(
            result.best_move,
            Some(ChessMove::new(Square::E2, Square::E4, None))
        );
        assert_eq!(result.depth, 0);
        assert_eq!(result.nodes, 0);
    }

    #[test]
    fn hybrid_search_respects_candidate_and_depth_limits() {
        let board = Board::default();
        let policy = policy_from_logits(&board, &[0.0; POLICY_MOVES], [0.0; 3], 1.0);
        let level = profile(3);
        let mut search = Search::new(
            SearchLimits {
                depth: Some(20),
                ..SearchLimits::default()
            },
            atomic_stop(),
        );
        let result = search.search_with_policy(
            board,
            &[],
            0,
            HybridConfig {
                policy: Some(&policy),
                level_profile: level,
                random_value: 0.5,
            },
            |_| {},
        );
        assert!(result.depth <= level.max_depth);
        assert!(result.root_moves.len() <= level.candidate_count);
        assert!(
            result
                .best_move
                .is_some_and(|chess_move| board.legal(chess_move))
        );
    }
}
