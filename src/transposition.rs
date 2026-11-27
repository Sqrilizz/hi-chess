use chess::ChessMove;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy, Debug)]
pub struct TTEntry {
    pub hash: u64,
    pub score: i32,
    pub depth: i16,
    pub bound: Bound,
    pub best_move: Option<ChessMove>,
}

impl Default for TTEntry {
    fn default() -> Self {
        Self {
            hash: 0,
            score: 0,
            depth: -1,
            bound: Bound::Exact,
            best_move: None,
        }
    }
}

#[derive(Debug)]
pub struct TranspositionTable {
    entries: Vec<TTEntry>,
    mask: usize,
}

impl TranspositionTable {
    pub fn new(entries: usize) -> Self {
        debug_assert!(entries.is_power_of_two());
        let entries = entries.next_power_of_two();
        Self {
            entries: vec![TTEntry::default(); entries],
            mask: entries - 1,
        }
    }

    pub fn probe(&self, hash: u64) -> Option<TTEntry> {
        let entry = self.entries[hash as usize & self.mask];
        if entry.depth >= 0 && entry.hash == hash {
            Some(entry)
        } else {
            None
        }
    }

    pub fn store(
        &mut self,
        hash: u64,
        score: i32,
        depth: usize,
        bound: Bound,
        best_move: Option<ChessMove>,
    ) {
        let index = hash as usize & self.mask;
        let existing = self.entries[index];
        if existing.depth >= 0 && existing.hash == hash && existing.depth > depth as i16 {
            return;
        }
        if existing.depth > depth as i16 && existing.best_move.is_some() {
            return;
        }

        self.entries[index] = TTEntry {
            hash,
            score,
            depth: depth as i16,
            bound,
            best_move,
        };
    }

    pub fn clear(&mut self) {
        self.entries.fill(TTEntry::default());
    }
}

#[cfg(test)]
mod tests {
    use super::{Bound, TranspositionTable};
    use chess::{ChessMove, Square};

    #[test]
    fn stores_and_probes_entries() {
        let mut table = TranspositionTable::new(16);
        let chess_move = ChessMove::new(Square::E2, Square::E4, None);
        table.store(42, 123, 5, Bound::Exact, Some(chess_move));
        let entry = table.probe(42).unwrap();
        assert_eq!(entry.score, 123);
        assert_eq!(entry.depth, 5);
        assert_eq!(entry.bound, Bound::Exact);
        assert_eq!(entry.best_move, Some(chess_move));
    }

    #[test]
    fn missing_entry_returns_none() {
        let table = TranspositionTable::new(16);
        assert!(table.probe(42).is_none());
    }

    #[test]
    fn deeper_entry_is_not_replaced_by_shallower_search() {
        let mut table = TranspositionTable::new(16);
        table.store(42, 100, 8, Bound::Exact, None);
        table.store(42, 50, 4, Bound::Lower, None);
        assert_eq!(table.probe(42).unwrap().depth, 8);
        assert_eq!(table.probe(42).unwrap().score, 100);
    }

    #[test]
    fn clear_removes_entries() {
        let mut table = TranspositionTable::new(16);
        table.store(42, 100, 8, Bound::Exact, None);
        table.clear();
        assert!(table.probe(42).is_none());
    }
}
