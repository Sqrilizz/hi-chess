use crate::features::{
    BOARD_SQUARES, INPUT_BYTES, PLANE_SIDE_TO_MOVE, POLICY_MOVES, encode_board, move_index,
};
use chess::{Board, ChessMove, MoveGen};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"HIDATA01";
const FORMAT_VERSION: u32 = 2;
pub const LEGAL_MASK_BYTES: usize = POLICY_MOVES / 8;
pub const RECORD_BYTES: usize = INPUT_BYTES + LEGAL_MASK_BYTES + 3;

#[derive(Clone, Debug)]
pub struct TrainingRecord {
    pub input: [u8; INPUT_BYTES],
    pub legal_mask: [u8; LEGAL_MASK_BYTES],
    pub target_move: u16,
    pub result: u8,
}

impl TrainingRecord {
    pub fn new(
        board: &Board,
        own_elo: u16,
        opponent_elo: u16,
        halfmove_clock: u16,
        last_move: Option<ChessMove>,
        played_move: ChessMove,
        result: u8,
    ) -> Result<Self, String> {
        if result > 2 {
            return Err("result must be between 0 and 2".to_string());
        }
        if !board.legal(played_move) {
            return Err("played move is not legal".to_string());
        }

        let mut legal_mask = [0; LEGAL_MASK_BYTES];
        for chess_move in MoveGen::new_legal(board) {
            let index = move_index(board, chess_move);
            legal_mask[index / 8] |= 1 << (index % 8);
        }

        Ok(Self {
            input: encode_board(board, own_elo, opponent_elo, halfmove_clock, last_move),
            legal_mask,
            target_move: move_index(board, played_move) as u16,
            result,
        })
    }

    pub fn to_bytes(&self) -> [u8; RECORD_BYTES] {
        let mut bytes = [0; RECORD_BYTES];
        bytes[..INPUT_BYTES].copy_from_slice(&self.input);
        bytes[INPUT_BYTES..INPUT_BYTES + LEGAL_MASK_BYTES].copy_from_slice(&self.legal_mask);
        let target_start = INPUT_BYTES + LEGAL_MASK_BYTES;
        bytes[target_start..target_start + 2].copy_from_slice(&self.target_move.to_le_bytes());
        bytes[target_start + 2] = self.result;
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, std::io::Error> {
        if bytes.len() != RECORD_BYTES {
            return Err(invalid_data("unexpected training record size"));
        }

        let mut input = [0; INPUT_BYTES];
        input.copy_from_slice(&bytes[..INPUT_BYTES]);
        let mut legal_mask = [0; LEGAL_MASK_BYTES];
        legal_mask.copy_from_slice(&bytes[INPUT_BYTES..INPUT_BYTES + LEGAL_MASK_BYTES]);
        let target_start = INPUT_BYTES + LEGAL_MASK_BYTES;
        let target_move = u16::from_le_bytes([bytes[target_start], bytes[target_start + 1]]);
        let result = bytes[target_start + 2];
        if target_move as usize >= POLICY_MOVES || result > 2 {
            return Err(invalid_data("invalid training record values"));
        }

        Ok(Self {
            input,
            legal_mask,
            target_move,
            result,
        })
    }

    fn from_legacy_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut record = Self::from_bytes(bytes)?;
        if record.input[PLANE_SIDE_TO_MOVE * BOARD_SQUARES] == 0 {
            record.target_move = mirror_policy_index(record.target_move as usize) as u16;
            let mut oriented_mask = [0; LEGAL_MASK_BYTES];
            for (byte_index, byte) in record.legal_mask.iter().copied().enumerate() {
                let mut remaining = byte;
                while remaining != 0 {
                    let bit = remaining.trailing_zeros() as usize;
                    let oriented = mirror_policy_index(byte_index * 8 + bit);
                    oriented_mask[oriented / 8] |= 1 << (oriented % 8);
                    remaining &= remaining - 1;
                }
            }
            record.legal_mask = oriented_mask;
        }
        Ok(record)
    }
}

fn mirror_policy_index(index: usize) -> usize {
    let source = index / BOARD_SQUARES;
    let dest = index % BOARD_SQUARES;
    (source ^ 56) * BOARD_SQUARES + (dest ^ 56)
}

pub fn write_records(path: impl AsRef<Path>, records: &[TrainingRecord]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(MAGIC)?;
    file.write_all(&FORMAT_VERSION.to_le_bytes())?;
    file.write_all(&(RECORD_BYTES as u32).to_le_bytes())?;
    file.write_all(&(records.len() as u64).to_le_bytes())?;
    for record in records {
        file.write_all(&record.to_bytes())?;
    }
    file.flush()
}

pub fn read_records(path: impl AsRef<Path>) -> std::io::Result<Vec<TrainingRecord>> {
    let mut file = File::open(path)?;
    let mut header = [0; 24];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid_data("invalid dataset magic"));
    }

    let version = u32::from_le_bytes(
        header[8..12]
            .try_into()
            .map_err(|_| invalid_data("invalid dataset header"))?,
    );
    let record_bytes = u32::from_le_bytes(
        header[12..16]
            .try_into()
            .map_err(|_| invalid_data("invalid dataset header"))?,
    );
    let count = u64::from_le_bytes(
        header[16..24]
            .try_into()
            .map_err(|_| invalid_data("invalid dataset header"))?,
    );
    if version == 0 || version > FORMAT_VERSION {
        return Err(invalid_data("unsupported dataset version"));
    }
    if record_bytes as usize != RECORD_BYTES {
        return Err(invalid_data("unsupported dataset record size"));
    }

    let mut records = Vec::with_capacity(count as usize);
    let mut buffer = [0; RECORD_BYTES];
    for _ in 0..count {
        file.read_exact(&mut buffer)?;
        let record = if version == 1 {
            TrainingRecord::from_legacy_bytes(&buffer)?
        } else {
            TrainingRecord::from_bytes(&buffer)?
        };
        records.push(record);
    }
    Ok(records)
}

fn invalid_data(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{
        INPUT_BYTES, LEGAL_MASK_BYTES, RECORD_BYTES, TrainingRecord, read_records, write_records,
    };
    use chess::{Board, ChessMove, Square};
    use std::str::FromStr;

    #[test]
    fn record_roundtrip() {
        let board = Board::default();
        let record = TrainingRecord::new(
            &board,
            1500,
            1600,
            0,
            None,
            ChessMove::new(Square::E2, Square::E4, None),
            2,
        )
        .unwrap();
        let bytes = record.to_bytes();
        assert_eq!(bytes.len(), RECORD_BYTES);
        let restored = TrainingRecord::from_bytes(&bytes).unwrap();
        assert_eq!(restored.input, record.input);
        assert_eq!(restored.legal_mask, record.legal_mask);
        assert_eq!(restored.target_move, record.target_move);
        assert_eq!(restored.result, record.result);
    }

    #[test]
    fn file_roundtrip() {
        let board = Board::default();
        let record = TrainingRecord::new(
            &board,
            1500,
            1600,
            0,
            None,
            ChessMove::new(Square::D2, Square::D4, None),
            1,
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "hi-dataset-{}-{}.bin",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        write_records(&path, std::slice::from_ref(&record)).unwrap();
        let restored = read_records(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].target_move, record.target_move);
    }

    #[test]
    fn migrates_legacy_black_targets_to_oriented_coordinates() {
        let board = Board::from_str("4k3/4p3/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        let record = TrainingRecord::new(
            &board,
            1500,
            1600,
            0,
            None,
            ChessMove::new(Square::E7, Square::E5, None),
            2,
        )
        .unwrap();
        let absolute_index = 52 * 64 + 36;
        let mut bytes = record.to_bytes();
        let target_start = INPUT_BYTES + LEGAL_MASK_BYTES;
        bytes[target_start..target_start + 2]
            .copy_from_slice(&(absolute_index as u16).to_le_bytes());
        bytes[INPUT_BYTES..INPUT_BYTES + LEGAL_MASK_BYTES].fill(0);
        bytes[INPUT_BYTES + absolute_index / 8] |= 1 << (absolute_index % 8);

        let restored = TrainingRecord::from_legacy_bytes(&bytes).unwrap();
        assert_eq!(restored.target_move, (12 * 64 + 28) as u16);
        let legal_indices: Vec<usize> = restored
            .legal_mask
            .iter()
            .enumerate()
            .flat_map(|(byte_index, byte)| {
                (0..8)
                    .filter(move |bit| byte & (1 << bit) != 0)
                    .map(move |bit| byte_index * 8 + bit)
            })
            .collect();
        assert_eq!(legal_indices, vec![12 * 64 + 28]);
    }

    #[test]
    fn rejects_illegal_target() {
        let board = Board::default();
        let result = TrainingRecord::new(
            &board,
            1500,
            1600,
            0,
            None,
            ChessMove::new(Square::E2, Square::E5, None),
            1,
        );
        assert!(result.is_err());
    }
}
