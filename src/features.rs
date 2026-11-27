use chess::{Board, CastleRights, ChessMove, Color, Piece, Square};

pub const INPUT_CHANNELS: usize = 23;
pub const BOARD_SQUARES: usize = 64;
pub const INPUT_BYTES: usize = INPUT_CHANNELS * BOARD_SQUARES;
pub const POLICY_MOVES: usize = BOARD_SQUARES * BOARD_SQUARES;
pub const VALUE_CLASSES: usize = 3;
pub const ELO_MIN: u16 = 600;
pub const ELO_MAX: u16 = 2_200;
pub const HALFMOVE_SCALE: u16 = 100;

pub const PLANE_SIDE_TO_MOVE: usize = 12;
const PLANE_OWN_KINGSIDE: usize = 13;
const PLANE_OWN_QUEENSIDE: usize = 14;
const PLANE_ENEMY_KINGSIDE: usize = 15;
const PLANE_ENEMY_QUEENSIDE: usize = 16;
const PLANE_OWN_ELO: usize = 17;
const PLANE_ENEMY_ELO: usize = 18;
const PLANE_HALFMOVE: usize = 19;
const PLANE_EN_PASSANT: usize = 20;
const PLANE_LAST_MOVE_FROM: usize = 21;
const PLANE_LAST_MOVE_TO: usize = 22;

#[derive(Clone, Copy, Debug)]
pub struct PolicyMove {
    pub chess_move: ChessMove,
    pub probability: f32,
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub moves: Vec<PolicyMove>,
    pub wdl: [f32; VALUE_CLASSES],
}

pub fn encode_board(
    board: &Board,
    own_elo: u16,
    opponent_elo: u16,
    halfmove_clock: u16,
    last_move: Option<ChessMove>,
) -> [u8; INPUT_BYTES] {
    let mut input = [0; INPUT_BYTES];
    let side_to_move = board.side_to_move();

    for (color_index, color) in [side_to_move, !side_to_move].into_iter().enumerate() {
        for piece in [
            Piece::Pawn,
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ] {
            let plane = color_index * 6 + piece.to_index();
            let squares = *board.pieces(piece) & *board.color_combined(color);
            for square in squares {
                let index = oriented_square_index(square, side_to_move);
                set_plane(&mut input, plane, index, 255);
            }
        }
    }

    if side_to_move == Color::White {
        set_full_plane(&mut input, PLANE_SIDE_TO_MOVE, 255);
    }

    let own_rights = board.castle_rights(side_to_move);
    let enemy_rights = board.castle_rights(!side_to_move);
    set_castling_rights(
        &mut input,
        own_rights,
        PLANE_OWN_KINGSIDE,
        PLANE_OWN_QUEENSIDE,
    );
    set_castling_rights(
        &mut input,
        enemy_rights,
        PLANE_ENEMY_KINGSIDE,
        PLANE_ENEMY_QUEENSIDE,
    );

    let own_elo = quantize_ratio(own_elo, ELO_MIN, ELO_MAX);
    let enemy_elo = quantize_ratio(opponent_elo, ELO_MIN, ELO_MAX);
    set_full_plane(&mut input, PLANE_OWN_ELO, own_elo);
    set_full_plane(&mut input, PLANE_ENEMY_ELO, enemy_elo);

    let halfmove = quantize_ratio(halfmove_clock, 0, HALFMOVE_SCALE);
    set_full_plane(&mut input, PLANE_HALFMOVE, halfmove);

    if let Some(en_passant_pawn) = board.en_passant() {
        let target = if side_to_move == Color::White {
            en_passant_pawn.up()
        } else {
            en_passant_pawn.down()
        };
        if let Some(target) = target {
            let index = oriented_square_index(target, side_to_move);
            set_plane(&mut input, PLANE_EN_PASSANT, index, 255);
        }
    }

    if let Some(chess_move) = last_move {
        let from = oriented_square_index(chess_move.get_source(), side_to_move);
        let to = oriented_square_index(chess_move.get_dest(), side_to_move);
        set_plane(&mut input, PLANE_LAST_MOVE_FROM, from, 255);
        set_plane(&mut input, PLANE_LAST_MOVE_TO, to, 255);
    }

    input
}

pub fn move_index(board: &Board, chess_move: ChessMove) -> usize {
    let source = oriented_square_index(chess_move.get_source(), board.side_to_move());
    let dest = oriented_square_index(chess_move.get_dest(), board.side_to_move());
    source * BOARD_SQUARES + dest
}

pub fn policy_from_logits(
    board: &Board,
    logits: &[f32; POLICY_MOVES],
    wdl: [f32; VALUE_CLASSES],
    temperature: f32,
) -> Policy {
    policy_from_logits_with_index(board, logits, wdl, temperature, move_index)
}

pub fn legacy_policy_from_logits(
    board: &Board,
    logits: &[f32; POLICY_MOVES],
    wdl: [f32; VALUE_CLASSES],
    temperature: f32,
) -> Policy {
    policy_from_logits_with_index(board, logits, wdl, temperature, absolute_move_index)
}

fn policy_from_logits_with_index(
    board: &Board,
    logits: &[f32; POLICY_MOVES],
    wdl: [f32; VALUE_CLASSES],
    temperature: f32,
    index_move: fn(&Board, ChessMove) -> usize,
) -> Policy {
    let temperature = temperature.max(0.05);
    let legal_moves: Vec<(ChessMove, f32)> = chess::MoveGen::new_legal(board)
        .map(|chess_move| {
            (
                chess_move,
                logits[index_move(board, chess_move)] / temperature,
            )
        })
        .collect();
    if legal_moves.is_empty() {
        return Policy {
            moves: Vec::new(),
            wdl: softmax(wdl, 1.0),
        };
    }

    let max_logit = legal_moves
        .iter()
        .map(|(_, logit)| *logit)
        .fold(f32::NEG_INFINITY, f32::max);

    let mut probabilities: Vec<f32> = legal_moves
        .iter()
        .map(|(_, logit)| (*logit - max_logit).exp())
        .collect();
    let sum: f32 = probabilities.iter().sum();
    for probability in &mut probabilities {
        *probability /= sum;
    }

    let moves = legal_moves
        .into_iter()
        .zip(probabilities)
        .map(|((chess_move, _), probability)| PolicyMove {
            chess_move,
            probability,
        })
        .collect();

    Policy {
        moves,
        wdl: softmax(wdl, 1.0),
    }
}

pub fn sample_policy(policy: &Policy, random_value: f32) -> Option<ChessMove> {
    let mut cumulative = 0.0;
    let target = random_value.clamp(0.0, 1.0);

    for policy_move in &policy.moves {
        cumulative += policy_move.probability;
        if target < cumulative {
            return Some(policy_move.chess_move);
        }
    }

    policy
        .moves
        .last()
        .map(|policy_move| policy_move.chess_move)
}

pub fn sample_policy_filtered(
    policy: &Policy,
    allowed_moves: &[ChessMove],
    random_value: f32,
) -> Option<ChessMove> {
    if allowed_moves.is_empty() {
        return sample_policy(policy, random_value);
    }

    let total: f32 = policy
        .moves
        .iter()
        .filter(|policy_move| allowed_moves.contains(&policy_move.chess_move))
        .map(|policy_move| policy_move.probability)
        .sum();
    if total <= 0.0 {
        return allowed_moves.first().copied();
    }

    let mut cumulative = 0.0;
    let target = random_value.clamp(0.0, 1.0) * total;
    let mut last_move = None;
    for policy_move in &policy.moves {
        if !allowed_moves.contains(&policy_move.chess_move) {
            continue;
        }
        cumulative += policy_move.probability;
        last_move = Some(policy_move.chess_move);
        if target < cumulative {
            return Some(policy_move.chess_move);
        }
    }
    last_move
}

pub fn top_policy_moves(policy: &Policy, count: usize) -> Vec<PolicyMove> {
    let mut moves = policy.moves.clone();
    moves.sort_unstable_by(|left, right| {
        right
            .probability
            .partial_cmp(&left.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.chess_move.cmp(&left.chess_move))
    });
    moves.truncate(count);
    moves
}

fn set_castling_rights(
    input: &mut [u8; INPUT_BYTES],
    rights: CastleRights,
    kingside_plane: usize,
    queenside_plane: usize,
) {
    if rights == CastleRights::Both || rights == CastleRights::KingSide {
        set_full_plane(input, kingside_plane, 255);
    }
    if rights == CastleRights::Both || rights == CastleRights::QueenSide {
        set_full_plane(input, queenside_plane, 255);
    }
}

fn set_full_plane(input: &mut [u8; INPUT_BYTES], plane: usize, value: u8) {
    let start = plane * BOARD_SQUARES;
    input[start..start + BOARD_SQUARES].fill(value);
}

fn set_plane(input: &mut [u8; INPUT_BYTES], plane: usize, square: usize, value: u8) {
    input[plane * BOARD_SQUARES + square] = value;
}

fn absolute_move_index(_board: &Board, chess_move: ChessMove) -> usize {
    chess_move.get_source().to_index() * BOARD_SQUARES + chess_move.get_dest().to_index()
}

fn oriented_square_index(square: Square, perspective: Color) -> usize {
    let index = square.to_index();
    if perspective == Color::White {
        index
    } else {
        index ^ 56
    }
}

fn quantize_ratio(value: u16, minimum: u16, maximum: u16) -> u8 {
    let range = maximum.saturating_sub(minimum).max(1);
    let clamped = value.clamp(minimum, maximum);
    ((clamped - minimum) as u32 * 255 / range as u32) as u8
}

fn softmax(values: [f32; VALUE_CLASSES], temperature: f32) -> [f32; VALUE_CLASSES] {
    let temperature = temperature.max(0.05);
    let max = values.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let exponentials = values.map(|value| ((value - max) / temperature).exp());
    let sum = exponentials.iter().sum::<f32>();
    exponentials.map(|value| value / sum)
}

#[cfg(test)]
mod tests {
    use super::{
        INPUT_BYTES, INPUT_CHANNELS, POLICY_MOVES, encode_board, legacy_policy_from_logits,
        move_index, policy_from_logits, sample_policy, sample_policy_filtered, top_policy_moves,
    };
    use chess::{Board, ChessMove, MoveGen, Piece, Square};
    use std::str::FromStr;

    #[test]
    fn startpos_has_twenty_legal_moves() {
        let board = Board::default();
        let input = encode_board(&board, 1500, 1600, 0, None);
        assert_eq!(input.len(), INPUT_BYTES);
        assert_eq!(input.len(), INPUT_CHANNELS * 64);

        let logits = [0.0; POLICY_MOVES];
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        assert_eq!(policy.moves.len(), 20);
        assert!(
            (policy
                .moves
                .iter()
                .map(|entry| entry.probability)
                .sum::<f32>()
                - 1.0)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn own_and_enemy_elo_planes_are_encoded() {
        let board = Board::default();
        let low = encode_board(&board, 600, 2200, 0, None);
        let high = encode_board(&board, 2200, 600, 0, None);
        assert_eq!(low[17 * 64], 0);
        assert_eq!(low[18 * 64], 255);
        assert_eq!(high[17 * 64], 255);
        assert_eq!(high[18 * 64], 0);
    }

    #[test]
    fn move_index_matches_the_oriented_board() {
        let white = Board::default();
        let black = Board::from_str("4k3/4p3/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        let white_move = ChessMove::new(Square::E2, Square::E4, None);
        let black_move = ChessMove::new(Square::E7, Square::E5, None);
        assert_eq!(move_index(&white, white_move), 12 * 64 + 28);
        assert_eq!(move_index(&black, black_move), 12 * 64 + 28);
    }

    #[test]
    fn black_policy_uses_oriented_logits() {
        let board = Board::from_str("4k3/4p3/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        let black_move = ChessMove::new(Square::E7, Square::E5, None);
        let white_move = ChessMove::new(Square::E2, Square::E4, None);
        let mut logits = [0.0; POLICY_MOVES];
        logits[move_index(&board, black_move)] = 10.0;
        logits[move_index(&board, white_move)] = -10.0;
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        assert_eq!(top_policy_moves(&policy, 1)[0].chess_move, black_move);
    }

    #[test]
    fn legacy_policy_keeps_absolute_black_coordinates() {
        let board = Board::from_str("4k3/4p3/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        let black_move = ChessMove::new(Square::E7, Square::E5, None);
        let mut logits = [0.0; POLICY_MOVES];
        logits[52 * 64 + 36] = 10.0;
        let policy = legacy_policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        assert_eq!(top_policy_moves(&policy, 1)[0].chess_move, black_move);
    }

    #[test]
    fn sampling_returns_a_legal_move() {
        let board = Board::default();
        let logits = [0.0; POLICY_MOVES];
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        let sampled = sample_policy(&policy, 0.5).unwrap();
        assert!(board.legal(sampled));
    }

    #[test]
    fn filtered_sampling_only_returns_allowed_moves() {
        let board = Board::default();
        let mut logits = [0.0; POLICY_MOVES];
        logits[move_index(&board, ChessMove::new(Square::E2, Square::E4, None))] = 10.0;
        logits[move_index(&board, ChessMove::new(Square::D2, Square::D4, None))] = 5.0;
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        let sampled = sample_policy_filtered(
            &policy,
            &[ChessMove::new(Square::D2, Square::D4, None)],
            0.1,
        )
        .unwrap();
        assert_eq!(sampled, ChessMove::new(Square::D2, Square::D4, None));
    }

    #[test]
    fn top_moves_respects_requested_count() {
        let board = Board::default();
        let mut logits = [0.0; POLICY_MOVES];
        logits[move_index(&board, ChessMove::new(Square::E2, Square::E4, None))] = 10.0;
        let policy = policy_from_logits(&board, &logits, [0.0; 3], 1.0);
        let top = top_policy_moves(&policy, 3);
        assert_eq!(top.len(), 3);
        assert_eq!(
            top[0].chess_move,
            ChessMove::new(Square::E2, Square::E4, None)
        );
    }

    #[test]
    fn encodes_side_to_move_and_castling_planes() {
        let board = Board::from_str("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let input = encode_board(&board, 1500, 1500, 0, None);
        assert_eq!(input[12 * 64], 255);
        for plane in 13..=16 {
            assert_eq!(input[plane * 64], 255);
        }
        assert_eq!(MoveGen::new_legal(&board).count(), 26);
    }

    #[test]
    fn promotion_variants_share_policy_index() {
        let board = Board::from_str("7k/P7/8/8/8/8/7p/7K w - - 0 1").unwrap();
        let queen = ChessMove::new(Square::A7, Square::A8, Some(Piece::Queen));
        let knight = ChessMove::new(Square::A7, Square::A8, Some(Piece::Knight));
        assert_eq!(move_index(&board, queen), move_index(&board, knight));
    }
}
