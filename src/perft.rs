use chess::{Board, MoveGen};

pub fn perft(board: &Board, depth: usize) -> u64 {
    if depth == 0 {
        1
    } else {
        MoveGen::movegen_perft_test(board, depth) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::perft;
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn startpos_counts() {
        let board = Board::default();
        assert_eq!(perft(&board, 0), 1);
        assert_eq!(perft(&board, 1), 20);
        assert_eq!(perft(&board, 2), 400);
        assert_eq!(perft(&board, 3), 8_902);
        assert_eq!(perft(&board, 4), 197_281);
    }

    #[test]
    fn kiwipete_counts() {
        let board =
            Board::from_str("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
                .unwrap();
        assert_eq!(perft(&board, 1), 48);
        assert_eq!(perft(&board, 2), 2_039);
        assert_eq!(perft(&board, 3), 97_862);
    }

    #[test]
    fn en_passant_and_promotion_counts() {
        let board =
            Board::from_str("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1")
                .unwrap();
        assert_eq!(perft(&board, 1), 6);
        assert_eq!(perft(&board, 2), 264);
        assert_eq!(perft(&board, 3), 9_467);
    }

    #[test]
    fn tactical_position_counts() {
        let board =
            Board::from_str("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8").unwrap();
        assert_eq!(perft(&board, 1), 44);
        assert_eq!(perft(&board, 2), 1_486);
        assert_eq!(perft(&board, 3), 62_379);
    }
}
