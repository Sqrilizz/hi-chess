use chess::{Board, Color, Piece, Square};

const PIECE_VALUES: [i32; 6] = [100, 320, 330, 500, 900, 0];
const TEMPO_BONUS: i32 = 10;
const BISHOP_PAIR_BONUS: i32 = 30;
const CASTLING_BONUS: i32 = 25;
const SEVENTH_RANK_BONUS: i32 = 25;
const ENDGAME_PIECE_COUNT: u32 = 12;

pub fn piece_value(piece: Piece) -> i32 {
    PIECE_VALUES[piece.to_index()]
}

pub fn evaluate(board: &Board) -> i32 {
    let white_score = evaluate_color(board, Color::White, is_endgame(board));
    let black_score = evaluate_color(board, Color::Black, is_endgame(board));
    let white_perspective = white_score - black_score;

    if board.side_to_move() == Color::White {
        white_perspective + TEMPO_BONUS
    } else {
        -white_perspective + TEMPO_BONUS
    }
}

fn evaluate_color(board: &Board, color: Color, endgame: bool) -> i32 {
    let mut score = 0;

    for piece in [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ] {
        let squares = *board.pieces(piece) & *board.color_combined(color);
        score += squares.popcnt() as i32 * piece_value(piece);

        for square in squares {
            score += square_bonus(piece, color, square, endgame);
        }
    }

    if (*board.pieces(Piece::Bishop) & *board.color_combined(color)).popcnt() >= 2 {
        score += BISHOP_PAIR_BONUS;
    }

    score
}

fn square_bonus(piece: Piece, color: Color, square: Square, endgame: bool) -> i32 {
    let square_index = square.to_index();
    let file = (square_index % 8) as i32;
    let rank = (square_index / 8) as i32;
    let relative_rank = if color == Color::White {
        rank
    } else {
        7 - rank
    };
    let center_distance = (file - 3).abs() + (relative_rank - 3).abs();

    match piece {
        Piece::Pawn => relative_rank * 6 - (file - 3).abs() * 2,
        Piece::Knight => (7 - center_distance) * 5,
        Piece::Bishop => (7 - center_distance) * 3,
        Piece::Rook => {
            let rank_bonus = if relative_rank == 6 {
                SEVENTH_RANK_BONUS
            } else {
                0
            };
            rank_bonus + relative_rank
        }
        Piece::Queen => 7 - center_distance,
        Piece::King => king_bonus(file, relative_rank, center_distance, endgame),
    }
}

fn king_bonus(file: i32, relative_rank: i32, center_distance: i32, endgame: bool) -> i32 {
    if endgame {
        return (7 - center_distance) * 10;
    }

    let castled = relative_rank == 0 && (file == 1 || file == 2 || file == 6 || file == 7);
    let castling_bonus = if castled { CASTLING_BONUS } else { 0 };
    castling_bonus - center_distance * 8
}

fn is_endgame(board: &Board) -> bool {
    if board.combined().popcnt() <= ENDGAME_PIECE_COUNT {
        return true;
    }

    let queens = *board.pieces(Piece::Queen);
    queens.popcnt() == 0
}

#[cfg(test)]
mod tests {
    use super::evaluate;
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn startpos_is_symmetric() {
        let board = Board::default();
        assert_eq!(evaluate(&board), 10);
    }

    #[test]
    fn extra_queen_improves_white_evaluation() {
        let board = Board::from_str("4k3/8/8/3Q4/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(evaluate(&board) > 900);
    }
}
