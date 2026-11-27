use crate::search::{Search, SearchLimits};
use chess::Board;
use std::{
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

const DEFAULT_DEPTH: u16 = 7;
const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const KIWIPETE_FEN: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
const ENDGAME_FEN: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";

#[derive(Clone, Debug)]
pub struct BenchOptions {
    pub depth: u16,
}

impl BenchOptions {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut depth = DEFAULT_DEPTH;
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].as_str() {
                "--depth" => {
                    index += 1;
                    let value = arguments
                        .get(index)
                        .ok_or_else(|| "missing value for --depth".to_string())?;
                    depth = value
                        .parse()
                        .map_err(|error| format!("invalid depth: {error}"))?;
                }
                unknown => return Err(format!("unknown bench option: {unknown}")),
            }
            index += 1;
        }
        if depth == 0 {
            return Err("depth must be positive".to_string());
        }
        Ok(Self { depth })
    }
}

pub fn run(options: &BenchOptions) -> Result<(), String> {
    let positions = [
        ("startpos", START_FEN),
        ("kiwipete", KIWIPETE_FEN),
        ("endgame", ENDGAME_FEN),
    ];
    let mut total_nodes = 0;
    let mut total_time = Duration::ZERO;

    for (name, fen) in positions {
        let board = Board::from_str(fen).map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut search = Search::new(
            SearchLimits {
                depth: Some(options.depth),
                ..SearchLimits::default()
            },
            stop,
        );
        let result = search.search(board, &[], 0, |_| {});
        let nps = nodes_per_second(result.nodes, result.elapsed);
        println!(
            "{name}: depth {} nodes {} time {} ms nps {nps} score {} bestmove {}",
            result.depth,
            result.nodes,
            result.elapsed.as_millis(),
            result.score,
            result
                .best_move
                .map(|chess_move| chess_move.to_string())
                .unwrap_or_else(|| "(none)".to_string())
        );
        total_nodes += result.nodes;
        total_time += result.elapsed;
    }

    let total_ms = milliseconds(total_time);
    let nps = nodes_per_second(total_nodes, total_time);
    println!("total: nodes {total_nodes} time {total_ms} ms nps {nps}");
    Ok(())
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn nodes_per_second(nodes: u64, elapsed: Duration) -> u64 {
    nodes
        .saturating_mul(1_000)
        .checked_div(milliseconds(elapsed))
        .unwrap_or(nodes)
}

#[cfg(test)]
mod tests {
    use super::{BenchOptions, ENDGAME_FEN, KIWIPETE_FEN, START_FEN};
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn parses_depth() {
        let arguments = ["--depth", "6"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(BenchOptions::parse(&arguments).unwrap().depth, 6);
    }

    #[test]
    fn rejects_zero_depth() {
        let arguments = ["--depth", "0"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(BenchOptions::parse(&arguments).is_err());
    }

    #[test]
    fn default_positions_are_valid() {
        for fen in [START_FEN, KIWIPETE_FEN, ENDGAME_FEN] {
            assert!(Board::from_str(fen).is_ok());
        }
    }
}
