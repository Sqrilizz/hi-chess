use crate::{
    hybrid::HybridConfig,
    inference::ModelRunner,
    level,
    options::UciOptions,
    search::{
        MATE_SCORE, MATE_THRESHOLD, Search, SearchInfo, SearchLimits, SearchResult,
        halfmove_clock_after,
    },
};
use chess::{Board, ChessMove, Color};
use rand::Rng;
use std::{
    fmt::Display,
    io::{self, BufRead, Write},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const ENGINE_NAME: &str = "HI";
const ENGINE_AUTHOR: &str = "sqrilizz";
const TIME_DIVISOR: u64 = 20;

#[derive(Clone, Debug)]
struct PositionState {
    board: Board,
    hashes: Vec<u64>,
    halfmove_clock: u16,
    last_move: Option<ChessMove>,
}

impl Default for PositionState {
    fn default() -> Self {
        let board = Board::default();
        Self {
            board,
            hashes: vec![board.get_hash()],
            halfmove_clock: 0,
            last_move: None,
        }
    }
}

#[derive(Debug)]
enum Event {
    Command(String),
    InputClosed,
    Info(SearchInfo, u64),
    Finished(SearchResult, u64),
}

struct Engine {
    position: PositionState,
    options: UciOptions,
    model: Option<ModelRunner>,
    sender: mpsc::Sender<Event>,
    stop: Option<Arc<AtomicBool>>,
    searching: bool,
    search_id: u64,
    closing: bool,
    stop_on_input_close: bool,
}

impl Engine {
    fn new(sender: mpsc::Sender<Event>) -> Self {
        Self {
            position: PositionState::default(),
            options: UciOptions::default(),
            model: None,
            sender,
            stop: None,
            searching: false,
            search_id: 0,
            closing: false,
            stop_on_input_close: false,
        }
    }

    fn reset_search(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.searching = false;
        self.search_id = self.search_id.wrapping_add(1);
    }

    fn request_stop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn ensure_model(&mut self) {
        if !self.options.use_model || self.options.level == 5 {
            self.model = None;
            return;
        }
        if self.model.is_some() {
            return;
        }

        match ModelRunner::new(&self.options.model_path, false) {
            Ok(model) => {
                emit("info string neural model loaded");
                self.model = Some(model);
            }
            Err(error) => emit(&format!(
                "info string neural model unavailable: {error}; using search only"
            )),
        }
    }

    fn start_search(&mut self, command: &GoCommand, sender: mpsc::Sender<Event>) {
        if self.searching {
            emit("info string search is already active");
            return;
        }

        self.ensure_model();
        let limits = SearchLimits {
            depth: command.depth,
            nodes: command.nodes,
            movetime: command.time_budget(self.position.board.side_to_move()),
        };
        let board = self.position.board;
        let prior_hashes = self.position.hashes[..self.position.hashes.len() - 1].to_vec();
        let halfmove_clock = self.position.halfmove_clock;
        let level_profile = if self.model.is_some() {
            level::profile(self.options.level)
        } else {
            if self.options.level != 5 {
                emit("info string neural levels require a model; using level 5 search");
            }
            level::profile(5)
        };
        let policy = if self.options.level == 5 {
            None
        } else {
            self.model.as_ref().and_then(|model| {
                match model.predict(
                    &board,
                    level_profile.own_elo,
                    self.options.opponent_elo,
                    halfmove_clock,
                    self.position.last_move,
                    level_profile.temperature,
                ) {
                    Ok(policy) => Some(policy),
                    Err(error) => {
                        emit(&format!("info string model inference failed: {error}"));
                        None
                    }
                }
            })
        };
        let effective_profile = if policy.is_some() {
            level_profile
        } else {
            level::profile(5)
        };
        let random_value = rand::rng().random::<f32>();
        let stop = Arc::new(AtomicBool::new(false));
        let search_stop = Arc::clone(&stop);
        self.stop = Some(stop);
        self.searching = true;
        self.stop_on_input_close =
            limits.depth.is_none() && limits.nodes.is_none() && limits.movetime.is_none();
        self.search_id = self.search_id.wrapping_add(1);
        let search_id = self.search_id;
        let event_sender = sender.clone();

        thread::spawn(move || {
            let mut search = Search::new(limits, search_stop);
            let result = search.search_with_policy(
                board,
                &prior_hashes,
                halfmove_clock,
                HybridConfig {
                    policy: policy.as_ref(),
                    level_profile: effective_profile,
                    random_value,
                },
                |info| {
                    let _ = event_sender.send(Event::Info(info, search_id));
                },
            );
            let _ = event_sender.send(Event::Finished(result, search_id));
        });
    }

    fn handle_command(&mut self, line: &str, sender: mpsc::Sender<Event>) -> bool {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(command) = tokens.first().copied() else {
            return true;
        };

        match command {
            "uci" => {
                emit(&format!("id name {ENGINE_NAME}"));
                emit(&format!("id author {ENGINE_AUTHOR}"));
                for option in self.options.declaration_lines() {
                    emit(&option);
                }
                emit("uciok");
            }
            "isready" => {
                self.ensure_model();
                emit("readyok");
            }
            "ucinewgame" => {
                self.reset_search();
                self.position = PositionState::default();
            }
            "position" => {
                self.reset_search();
                match parse_position(&tokens) {
                    Ok(position) => self.position = position,
                    Err(error) => emit(&format!("info string invalid position: {error}")),
                }
            }
            "go" => {
                self.reset_search();
                match GoCommand::parse(&tokens) {
                    Ok(command) => self.start_search(&command, sender),
                    Err(error) => emit(&format!("info string invalid go command: {error}")),
                }
            }
            "stop" => self.request_stop(),
            "quit" => {
                self.request_stop();
                return false;
            }
            "setoption" => {
                let reload_model = tokens.get(2).copied() == Some("UseModel")
                    || tokens.get(2).copied() == Some("ModelPath");
                match self.options.apply_setoption(&tokens) {
                    Ok(()) => {
                        if reload_model {
                            self.model = None;
                        }
                    }
                    Err(error) => emit(&format!("info string invalid setoption: {error}")),
                }
            }
            "d" => {
                for board_line in self.position.board.to_string().lines() {
                    emit(&format!("info string {board_line}"));
                }
            }
            "ponderhit" | "register" => {}
            _ => emit(&format!("info string unknown command: {command}")),
        }

        true
    }

    fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Command(line) => return self.handle_command(&line, self.sender.clone()),
            Event::InputClosed => {
                if self.searching {
                    self.closing = true;
                    if self.stop_on_input_close {
                        self.request_stop();
                    }
                } else {
                    return false;
                }
            }
            Event::Info(info, search_id) => {
                if search_id == self.search_id {
                    print_search_info(&info);
                }
            }
            Event::Finished(result, search_id) => {
                if search_id != self.search_id {
                    return true;
                }

                self.searching = false;
                self.stop = None;
                print_search_result(&result);
                if self.closing {
                    return false;
                }
            }
        }

        true
    }
}

pub fn run() {
    let (sender, receiver) = mpsc::channel();
    let input_sender = sender.clone();
    let input_thread = thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if input_sender.send(Event::Command(line)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = input_sender.send(Event::InputClosed);
    });

    let mut engine = Engine::new(sender.clone());
    while let Ok(event) = receiver.recv() {
        if !engine.handle_event(event) {
            break;
        }
    }

    engine.request_stop();
    drop(input_thread);
}

#[derive(Default, Debug)]
struct GoCommand {
    depth: Option<u16>,
    nodes: Option<u64>,
    movetime: Option<u64>,
    wtime: Option<u64>,
    btime: Option<u64>,
    winc: Option<u64>,
    binc: Option<u64>,
    movestogo: Option<u64>,
    infinite: bool,
    ponder: bool,
}

impl GoCommand {
    fn parse(tokens: &[&str]) -> Result<Self, String> {
        let mut command = Self {
            infinite: tokens.len() == 1,
            ..Self::default()
        };
        let mut index = 1;

        while index < tokens.len() {
            match tokens[index] {
                "depth" => command.depth = Some(parse_number(tokens, &mut index, "depth")?),
                "nodes" => command.nodes = Some(parse_number(tokens, &mut index, "nodes")?),
                "movetime" => {
                    command.movetime = Some(parse_number(tokens, &mut index, "movetime")?)
                }
                "wtime" => command.wtime = Some(parse_number(tokens, &mut index, "wtime")?),
                "btime" => command.btime = Some(parse_number(tokens, &mut index, "btime")?),
                "winc" => command.winc = Some(parse_number(tokens, &mut index, "winc")?),
                "binc" => command.binc = Some(parse_number(tokens, &mut index, "binc")?),
                "movestogo" => {
                    command.movestogo = Some(parse_number(tokens, &mut index, "movestogo")?)
                }
                "infinite" => command.infinite = true,
                "ponder" => command.ponder = true,
                unknown => return Err(format!("unknown token {unknown}")),
            }
            index += 1;
        }

        if command.depth == Some(0) || command.nodes == Some(0) || command.movetime == Some(0) {
            return Err("depth, nodes and movetime must be positive".to_string());
        }
        if command.movestogo == Some(0) {
            return Err("movestogo must be positive".to_string());
        }

        Ok(command)
    }

    fn time_budget(&self, color: Color) -> Option<Duration> {
        if self.infinite || self.ponder {
            return None;
        }
        if let Some(movetime) = self.movetime {
            return Some(Duration::from_millis(movetime));
        }

        let remaining = match color {
            Color::White => self.wtime,
            Color::Black => self.btime,
        }?;
        let increment = match color {
            Color::White => self.winc.unwrap_or(0),
            Color::Black => self.binc.unwrap_or(0),
        };
        let base = if let Some(moves_to_go) = self.movestogo {
            remaining / moves_to_go.max(1)
        } else {
            remaining / TIME_DIVISOR
        };
        let budget = (base + increment).clamp(1, remaining.saturating_sub(1).max(1));
        Some(Duration::from_millis(budget))
    }
}

fn parse_number<T>(tokens: &[&str], index: &mut usize, name: &str) -> Result<T, String>
where
    T: FromStr,
    T::Err: Display,
{
    *index += 1;
    let token = tokens
        .get(*index)
        .ok_or_else(|| format!("missing value for {name}"))?;
    token
        .parse()
        .map_err(|error| format!("invalid value for {name}: {error}"))
}

fn parse_position(tokens: &[&str]) -> Result<PositionState, String> {
    let mut moves_start;
    let (board, halfmove_clock) = match tokens.get(1).copied() {
        Some("startpos") => {
            moves_start = 2;
            (Board::default(), 0)
        }
        Some("fen") => {
            if tokens.len() < 8 {
                return Err("FEN requires six fields".to_string());
            }
            let fen = tokens[2..8].join(" ");
            let board = Board::from_str(&fen).map_err(|error| error.to_string())?;
            let halfmove_clock = tokens[6]
                .parse::<u16>()
                .map_err(|error| format!("invalid halfmove clock: {error}"))?;
            moves_start = 8;
            (board, halfmove_clock)
        }
        _ => return Err("expected startpos or fen".to_string()),
    };

    let mut position = PositionState {
        board,
        hashes: vec![board.get_hash()],
        halfmove_clock,
        last_move: None,
    };

    if tokens.get(moves_start) == Some(&"moves") {
        moves_start += 1;
    }

    for move_text in &tokens[moves_start..] {
        let chess_move = ChessMove::from_str(move_text)
            .map_err(|error| format!("invalid move {move_text}: {error}"))?;
        if !position.board.legal(chess_move) {
            return Err(format!("illegal move {move_text}"));
        }

        position.halfmove_clock =
            halfmove_clock_after(&position.board, position.halfmove_clock, chess_move);
        position.board = position.board.make_move_new(chess_move);
        position.hashes.push(position.board.get_hash());
        position.last_move = Some(chess_move);
    }

    Ok(position)
}

fn print_search_info(info: &SearchInfo) {
    let elapsed_ms = info.elapsed.as_millis();
    let nps = if elapsed_ms == 0 {
        info.nodes
    } else {
        info.nodes.saturating_mul(1_000) / elapsed_ms as u64
    };
    emit(&format!(
        "info depth {} seldepth {} score {} nodes {} time {} nps {} pv {}",
        info.depth,
        info.selective_depth,
        format_score(info.score),
        info.nodes,
        elapsed_ms,
        nps,
        info.best_move
    ));
}

fn print_search_result(result: &SearchResult) {
    let best_move = result
        .best_move
        .map(|chess_move| chess_move.to_string())
        .unwrap_or_else(|| "(none)".to_string());
    emit(&format!("bestmove {best_move}"));
}

fn format_score(score: i32) -> String {
    if score > MATE_THRESHOLD {
        let moves = (MATE_SCORE - score + 1) / 2;
        format!("mate {moves}")
    } else if score < -MATE_THRESHOLD {
        let moves = (MATE_SCORE + score + 1) / 2;
        format!("mate -{moves}")
    } else {
        format!("cp {score}")
    }
}

fn emit(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::{GoCommand, parse_position};
    use chess::Color;

    #[test]
    fn parses_startpos_with_moves() {
        let position =
            parse_position(&["position", "startpos", "moves", "e2e4", "e7e5", "g1f3"]).unwrap();
        assert_eq!(position.board.side_to_move(), Color::Black);
        assert_eq!(position.hashes.len(), 4);
        assert_eq!(position.halfmove_clock, 1);
    }

    #[test]
    fn parses_fen_and_updates_halfmove_clock() {
        let position = parse_position(&[
            "position",
            "fen",
            "4k3/8/8/8/8/8/8/4K2R",
            "w",
            "K",
            "-",
            "42",
            "43",
            "moves",
            "h1h2",
        ])
        .unwrap();
        assert_eq!(position.board.side_to_move(), Color::Black);
        assert_eq!(position.halfmove_clock, 43);
    }

    #[test]
    fn rejects_illegal_moves() {
        let result = parse_position(&["position", "startpos", "moves", "e2e5"]);
        assert!(result.is_err());
    }

    #[test]
    fn parses_go_depth() {
        let command = GoCommand::parse(&["go", "depth", "7"]).unwrap();
        assert_eq!(command.depth, Some(7));
        assert_eq!(command.time_budget(Color::White), None);
    }

    #[test]
    fn computes_clock_budget_with_increment() {
        let command = GoCommand::parse(&[
            "go", "wtime", "10000", "btime", "8000", "winc", "100", "binc", "200",
        ])
        .unwrap();
        assert_eq!(
            command.time_budget(Color::White),
            Some(std::time::Duration::from_millis(600))
        );
        assert_eq!(
            command.time_budget(Color::Black),
            Some(std::time::Duration::from_millis(600))
        );
    }

    #[test]
    fn bare_go_runs_until_stop() {
        let command = GoCommand::parse(&["go"]).unwrap();
        assert!(command.infinite);
        assert_eq!(command.time_budget(Color::White), None);
    }
}
