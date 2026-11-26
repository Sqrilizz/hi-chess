use crate::evaluation::piece_value;
use chess::{Board, ChessMove, MoveGen, Piece};

const PREFERRED_MOVE_SCORE: i32 = 2_000_000;
const CAPTURE_SCORE: i32 = 1_000_000;
const PROMOTION_SCORE: i32 = 900_000;
const KILLER_SCORE: i32 = 800_000;
const CAPTURE_VALUE_SCALE: i32 = 16;

pub fn is_capture(board: &Board, chess_move: ChessMove) -> bool {
    board.piece_on(chess_move.get_dest()).is_some() || is_en_passant(board, chess_move)
}

pub fn is_tactical(board: &Board, chess_move: ChessMove) -> bool {
    is_capture(board, chess_move) || chess_move.get_promotion().is_some()
}

pub fn order_moves(
    board: &Board,
    preferred: Option<ChessMove>,
    ply: usize,
    killers: &[[Option<ChessMove>; 2]],
    history: &[[i32; 64]; 64],
) -> Vec<ChessMove> {
    let ply_killers = killers.get(ply).copied().unwrap_or([None, None]);
    let mut scored_moves: Vec<(i32, ChessMove)> = MoveGen::new_legal(board)
        .map(|chess_move| {
            let score = move_score(board, chess_move, preferred, ply_killers, history);
            (score, chess_move)
        })
        .collect();

    scored_moves
        .sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    scored_moves
        .into_iter()
        .map(|(_, chess_move)| chess_move)
        .collect()
}

fn move_score(
    board: &Board,
    chess_move: ChessMove,
    preferred: Option<ChessMove>,
    killers: [Option<ChessMove>; 2],
    history: &[[i32; 64]; 64],
) -> i32 {
    if preferred == Some(chess_move) {
        return PREFERRED_MOVE_SCORE;
    }

    if let Some(promotion) = chess_move.get_promotion() {
        return PROMOTION_SCORE + piece_value(promotion);
    }

    if is_capture(board, chess_move) {
        let victim = captured_piece_value(board, chess_move);
        let attacker = board
            .piece_on(chess_move.get_source())
            .map(piece_value)
            .unwrap_or(0);
        return CAPTURE_SCORE + victim * CAPTURE_VALUE_SCALE - attacker;
    }

    if killers.contains(&Some(chess_move)) {
        return KILLER_SCORE;
    }

    history[chess_move.get_source().to_index()][chess_move.get_dest().to_index()]
}

fn captured_piece_value(board: &Board, chess_move: ChessMove) -> i32 {
    if is_en_passant(board, chess_move) {
        return piece_value(Piece::Pawn);
    }

    board
        .piece_on(chess_move.get_dest())
        .map(piece_value)
        .unwrap_or(0)
}

fn is_en_passant(board: &Board, chess_move: ChessMove) -> bool {
    if board.piece_on(chess_move.get_source()) != Some(Piece::Pawn)
        || board.piece_on(chess_move.get_dest()).is_some()
    {
        return false;
    }

    let Some(en_passant_pawn) = board.en_passant() else {
        return false;
    };
    let target = if board.side_to_move() == chess::Color::White {
        en_passant_pawn.up()
    } else {
        en_passant_pawn.down()
    };
    target == Some(chess_move.get_dest())
}

#[cfg(test)]
mod tests {
    use super::{is_capture, order_moves};
    use chess::{Board, ChessMove, Piece, Square};
    use std::str::FromStr;

    #[test]
    fn captures_are_ordered_before_quiets() {
        let board = Board::from_str("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let moves = order_moves(&board, None, 0, &[], &[[0; 64]; 64]);
        assert_eq!(moves[0], ChessMove::new(Square::E4, Square::D5, None));
    }

    #[test]
    fn en_passant_is_a_capture() {
        let board = Board::default()
            .make_move_new(ChessMove::new(Square::E2, Square::E4, None))
            .make_move_new(ChessMove::new(Square::A7, Square::A6, None))
            .make_move_new(ChessMove::new(Square::E4, Square::E5, None))
            .make_move_new(ChessMove::new(Square::D7, Square::D5, None));
        assert!(
            is_capture(&board, ChessMove::new(Square::E5, Square::D6, None)),
            "en passant pawn: {:?}, source piece: {:?}",
            board.en_passant(),
            board.piece_on(Square::E5)
        );
    }

    #[test]
    fn promotion_is_ordered_first() {
        let board = Board::from_str("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let moves = order_moves(&board, None, 0, &[], &[[0; 64]; 64]);
        assert_eq!(
            moves[0],
            ChessMove::new(Square::A7, Square::A8, Some(Piece::Queen))
        );
    }
}
