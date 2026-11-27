use crate::{
    evaluation::piece_value,
    personality::{Moment, line},
    search::{MATE_THRESHOLD, Search, SearchLimits, halfmove_clock_after},
};
use chess::{Board, BoardStatus, ChessMove, Color, Piece};
use serde::Serialize;
use std::{
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
};

const DEFAULT_DEPTH: u16 = 8;
const MAX_DEPTH: u16 = 32;
const SACRIFICE_THRESHOLD: i32 = 150;
const WINNING_SCORE: i32 = 250;
const MISSED_WIN_SCORE: i32 = 300;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoveLabel {
    Brilliant,
    Great,
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
}

#[derive(Clone, Debug, Serialize)]
pub struct Alternative {
    pub uci: String,
    pub score_cp: i32,
    pub win_probability: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct MoveReview {
    pub ply: usize,
    pub color: String,
    pub uci: String,
    pub fen_before: String,
    pub fen_after: String,
    pub best_move: String,
    pub alternatives: Vec<Alternative>,
    pub label: MoveLabel,
    pub centipawn_loss: i32,
    pub win_probability_before: f32,
    pub win_probability_after: f32,
    pub evaluation_before: i32,
    pub evaluation_after: i32,
    pub missed_win: bool,
    pub search_depth: usize,
    pub nodes: u64,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Accuracy {
    pub white: f32,
    pub black: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct GameReview {
    pub version: u32,
    pub initial_fen: String,
    pub player: String,
    pub requested_depth: u16,
    pub moves: Vec<MoveReview>,
    pub accuracy: Accuracy,
    pub result: Option<String>,
    pub closing_line: Option<String>,
}

pub fn review_game(
    initial_fen: &str,
    moves: &[ChessMove],
    depth: u16,
    player: Color,
) -> Result<GameReview, String> {
    if depth == 0 || depth > MAX_DEPTH {
        return Err(format!("depth must be between 1 and {MAX_DEPTH}"));
    }

    let initial_board = Board::from_str(initial_fen).map_err(|error| error.to_string())?;
    let (mut halfmove_clock, mut fullmove_number) = game_counters(initial_fen);
    let normalized_initial_fen = format_fen(&initial_board, halfmove_clock, fullmove_number);
    let mut board = initial_board;
    let mut hashes = vec![board.get_hash()];
    let mut reviews = Vec::with_capacity(moves.len());
    let mut white_accuracy = 0.0;
    let mut black_accuracy = 0.0;
    let mut white_moves = 0;
    let mut black_moves = 0;

    for (index, chess_move) in moves.iter().copied().enumerate() {
        if !board.legal(chess_move) {
            return Err(format!("illegal move at ply {}: {chess_move}", index + 1));
        }
        let side = board.side_to_move();
        let result = search_exact(&board, &hashes, halfmove_clock, depth)?;
        let best = result
            .root_moves
            .first()
            .copied()
            .ok_or_else(|| format!("analysis produced no root moves at ply {}", index + 1))?;
        let played = result
            .root_moves
            .iter()
            .copied()
            .find(|scored| scored.chess_move == chess_move)
            .ok_or_else(|| format!("played move missing from analysis at ply {}", index + 1))?;
        let centipawn_loss = best.score.saturating_sub(played.score).max(0);
        let second_score = result.root_moves.get(1).map(|scored| scored.score);
        let next_board = board.make_move_new(chess_move);
        let next_halfmove = halfmove_clock_after(&board, halfmove_clock, chess_move);
        let next_fullmove = if side == Color::Black {
            fullmove_number.saturating_add(1)
        } else {
            fullmove_number
        };
        let sacrifice =
            material_score(&next_board, side) + SACRIFICE_THRESHOLD < material_score(&board, side);
        let label = classify_move(
            chess_move == best.chess_move,
            centipawn_loss,
            best.score,
            second_score,
            sacrifice,
            result.root_moves.len(),
        );
        let missed_win = best.score >= MISSED_WIN_SCORE && played.score < WINNING_SCORE;
        let probability_before = white_win_probability(best.score, side);
        let probability_after = white_win_probability(-played.score, !side);
        let probability_loss = (probability_before - probability_after).max(0.0);
        let move_accuracy = 100.0 * (-3.0 * probability_loss).exp();
        if side == Color::White {
            white_accuracy += move_accuracy;
            white_moves += 1;
        } else {
            black_accuracy += move_accuracy;
            black_moves += 1;
        }
        let alternatives = result
            .root_moves
            .iter()
            .take(3)
            .map(|scored| Alternative {
                uci: scored.chess_move.to_string(),
                score_cp: white_score(scored.score, side),
                win_probability: white_win_probability(scored.score, side),
            })
            .collect();
        let comment = if side == player {
            Some(line(personality_moment(label), index / 2).to_string())
        } else {
            None
        };
        let fen_before = format_fen(&board, halfmove_clock, fullmove_number);
        let evaluation_before = white_score(best.score, side);
        let evaluation_after = white_score(-played.score, !side);
        let search_depth = result.depth;
        let nodes = result.nodes;

        reviews.push(MoveReview {
            ply: index + 1,
            color: color_name(side).to_string(),
            uci: chess_move.to_string(),
            fen_after: format_fen(&next_board, next_halfmove, next_fullmove),
            fen_before,
            best_move: best.chess_move.to_string(),
            alternatives,
            label,
            centipawn_loss,
            win_probability_before: probability_before,
            win_probability_after: probability_after,
            evaluation_before,
            evaluation_after,
            missed_win,
            search_depth,
            nodes,
            comment,
        });

        halfmove_clock = next_halfmove;
        fullmove_number = next_fullmove;
        board = next_board;
        hashes.push(board.get_hash());
    }

    let result = game_result(&board, halfmove_clock);
    let closing_line = result.as_deref().map(|result| closing_line(result, player));
    Ok(GameReview {
        version: 1,
        initial_fen: normalized_initial_fen,
        player: color_name(player).to_string(),
        requested_depth: depth,
        moves: reviews,
        accuracy: Accuracy {
            white: if white_moves == 0 {
                0.0
            } else {
                white_accuracy / white_moves as f32
            },
            black: if black_moves == 0 {
                0.0
            } else {
                black_accuracy / black_moves as f32
            },
        },
        result,
        closing_line,
    })
}

pub fn default_depth() -> u16 {
    DEFAULT_DEPTH
}

pub fn max_depth() -> u16 {
    MAX_DEPTH
}

fn search_exact(
    board: &Board,
    hashes: &[u64],
    halfmove_clock: u16,
    depth: u16,
) -> Result<crate::search::SearchResult, String> {
    let mut search = Search::new(
        SearchLimits {
            depth: Some(depth),
            ..SearchLimits::default()
        },
        Arc::new(AtomicBool::new(false)),
    );
    let result = search.search_exact(*board, &hashes[..hashes.len() - 1], halfmove_clock, |_| {});
    if result.depth == 0 || result.root_moves.is_empty() {
        return Err("analysis did not complete a search depth".to_string());
    }
    Ok(result)
}

fn classify_move(
    is_best: bool,
    centipawn_loss: i32,
    best_score: i32,
    second_score: Option<i32>,
    sacrifice: bool,
    legal_count: usize,
) -> MoveLabel {
    if is_best {
        let only_strong_move =
            second_score.is_some_and(|score| best_score.saturating_sub(score) >= 120);
        if legal_count > 1
            && (WINNING_SCORE..MATE_THRESHOLD).contains(&best_score)
            && (only_strong_move || sacrifice)
        {
            MoveLabel::Brilliant
        } else {
            MoveLabel::Best
        }
    } else if centipawn_loss <= 15 && best_score >= WINNING_SCORE {
        MoveLabel::Great
    } else if centipawn_loss <= 30 {
        MoveLabel::Good
    } else if centipawn_loss <= 80 {
        MoveLabel::Inaccuracy
    } else if centipawn_loss <= 180 {
        MoveLabel::Mistake
    } else {
        MoveLabel::Blunder
    }
}

fn material_score(board: &Board, color: Color) -> i32 {
    [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
    ]
    .into_iter()
    .map(|piece| {
        (*board.pieces(piece) & *board.color_combined(color)).popcnt() as i32 * piece_value(piece)
    })
    .sum()
}

fn white_win_probability(score: i32, side: Color) -> f32 {
    let probability = if score >= MATE_THRESHOLD {
        1.0
    } else if score <= -MATE_THRESHOLD {
        0.0
    } else {
        1.0 / (1.0 + (-f64::from(score) / 400.0).exp()) as f32
    };
    if side == Color::White {
        probability
    } else {
        1.0 - probability
    }
}

fn white_score(score: i32, side: Color) -> i32 {
    if side == Color::White { score } else { -score }
}

fn game_result(board: &Board, halfmove_clock: u16) -> Option<String> {
    match board.status() {
        BoardStatus::Checkmate => Some(if board.side_to_move() == Color::White {
            "0-1".to_string()
        } else {
            "1-0".to_string()
        }),
        BoardStatus::Stalemate => Some("1/2-1/2".to_string()),
        BoardStatus::Ongoing if halfmove_clock >= 100 => Some("1/2-1/2".to_string()),
        BoardStatus::Ongoing => None,
    }
}

fn personality_moment(label: MoveLabel) -> Moment {
    match label {
        MoveLabel::Brilliant => Moment::Brilliant,
        MoveLabel::Great | MoveLabel::Best => Moment::Best,
        MoveLabel::Good => Moment::Good,
        MoveLabel::Inaccuracy => Moment::Inaccuracy,
        MoveLabel::Mistake => Moment::Mistake,
        MoveLabel::Blunder => Moment::Blunder,
    }
}

fn closing_line(result: &str, player: Color) -> String {
    let moment = if result == "1/2-1/2" {
        Moment::Draw
    } else if (result == "1-0" && player == Color::White)
        || (result == "0-1" && player == Color::Black)
    {
        Moment::Victory
    } else {
        Moment::Defeat
    };
    line(moment, result.len()).to_string()
}

fn color_name(color: Color) -> &'static str {
    if color == Color::White {
        "white"
    } else {
        "black"
    }
}

fn game_counters(fen: &str) -> (u16, u16) {
    let mut fields = fen.split_whitespace();
    let halfmove_clock = fields
        .nth(4)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let fullmove_number = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    (halfmove_clock, fullmove_number)
}

fn format_fen(board: &Board, halfmove_clock: u16, fullmove_number: u16) -> String {
    let placement: String = board
        .to_string()
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{placement} {halfmove_clock} {fullmove_number}")
}

#[cfg(test)]
mod tests {
    use super::{MoveLabel, classify_move, review_game};
    use crate::search::MATE_SCORE;
    use chess::{Board, ChessMove, Color, Square};
    use std::str::FromStr;

    #[test]
    fn classifies_centipawn_loss() {
        assert_eq!(
            classify_move(false, 10, 100, Some(90), false, 20),
            MoveLabel::Good
        );
        assert_eq!(
            classify_move(false, 50, 100, Some(90), false, 20),
            MoveLabel::Inaccuracy
        );
        assert_eq!(
            classify_move(false, 120, 100, Some(90), false, 20),
            MoveLabel::Mistake
        );
        assert_eq!(
            classify_move(false, 250, 100, Some(90), false, 20),
            MoveLabel::Blunder
        );
    }

    #[test]
    fn does_not_call_every_best_move_brilliant() {
        assert_eq!(
            classify_move(true, 0, 400, Some(390), false, 20),
            MoveLabel::Best
        );
        assert_eq!(
            classify_move(true, 0, 400, Some(200), false, 20),
            MoveLabel::Brilliant
        );
        assert_eq!(
            classify_move(true, 0, MATE_SCORE, None, false, 1),
            MoveLabel::Best
        );
    }

    #[test]
    fn reviews_a_mate_in_one() {
        let board = Board::from_str("7k/8/5KQ1/8/8/8/8/8 w - - 0 1").unwrap();
        let mate = ChessMove::new(Square::G6, Square::G7, None);
        let review = review_game(&board.to_string(), &[mate], 2, Color::White).unwrap();
        assert_eq!(review.moves.len(), 1);
        assert_eq!(review.moves[0].label, MoveLabel::Best);
        assert_eq!(review.moves[0].best_move, "g6g7");
        assert_eq!(review.result.as_deref(), Some("1-0"));
    }

    #[test]
    fn rejects_illegal_moves() {
        let board = Board::default();
        let illegal = ChessMove::new(Square::E2, Square::E5, None);
        assert!(review_game(&board.to_string(), &[illegal], 2, Color::White).is_err());
    }
}
