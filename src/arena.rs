use crate::{
    search::{halfmove_clock_after, insufficient_material},
    uci_client::UciEngine,
};
use chess::{Board, BoardStatus, ChessMove, Color, Piece};
use std::{
    env,
    fmt::Display,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, Instant},
};

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const DEFAULT_OPENINGS: &str = "assets/openings.fen";
const DEFAULT_OUTPUT: &str = "matches/match.tsv";
const DEFAULT_MODEL: &str = "models/hi-v3-mixed.bpk";
const DEFAULT_GAMES: usize = 20;
const DEFAULT_LEVEL: u8 = 3;
const DEFAULT_ELO: u16 = 1_500;
const DEFAULT_MAX_PLIES: usize = 300;
const ENGINE_GRACE_MS: u64 = 5_000;
const CLOCK_FLAG_GRACE_MS: u64 = 250;
const ELO_K_FACTOR: f64 = 32.0;

#[derive(Clone, Debug)]
pub struct MatchOptions {
    pub hi_path: PathBuf,
    pub opponent_path: PathBuf,
    pub level: u8,
    pub use_model: bool,
    pub model_path: PathBuf,
    pub opponent_options: Vec<(String, String)>,
    pub games: usize,
    pub time_control: TimeControl,
    pub hi_elo: u16,
    pub opponent_elo: u16,
    pub openings_path: PathBuf,
    pub output_path: PathBuf,
    pub max_plies: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TimeControl {
    pub base_ms: u64,
    pub increment_ms: u64,
}

impl FromStr for TimeControl {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (base, increment) = value
            .split_once('+')
            .ok_or_else(|| "time control must look like 10+0.1".to_string())?;
        let base_seconds = base
            .parse::<f64>()
            .map_err(|error| format!("invalid base time: {error}"))?;
        let increment_seconds = increment
            .parse::<f64>()
            .map_err(|error| format!("invalid increment: {error}"))?;
        if !base_seconds.is_finite()
            || !increment_seconds.is_finite()
            || base_seconds <= 0.0
            || increment_seconds < 0.0
        {
            return Err("base time must be positive and increment non-negative".to_string());
        }
        Ok(Self {
            base_ms: (base_seconds * 1_000.0).round() as u64,
            increment_ms: (increment_seconds * 1_000.0).round() as u64,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameResult {
    WhiteWin,
    BlackWin,
    Draw,
}

impl GameResult {
    fn token(self) -> &'static str {
        match self {
            Self::WhiteWin => "1-0",
            Self::BlackWin => "0-1",
            Self::Draw => "1/2-1/2",
        }
    }

    fn hi_score(self, hi_color: Color) -> f64 {
        match self {
            Self::WhiteWin if hi_color == Color::White => 1.0,
            Self::BlackWin if hi_color == Color::Black => 1.0,
            Self::Draw => 0.5,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
struct GameRecord {
    index: usize,
    hi_color: Color,
    result: GameResult,
    reason: String,
    opening: String,
    moves: Vec<ChessMove>,
}

#[derive(Clone, Debug)]
pub struct MatchSummary {
    pub games: usize,
    pub hi_wins: usize,
    pub draws: usize,
    pub hi_losses: usize,
    pub score: f64,
    pub initial_hi_elo: u16,
    pub final_hi_elo: f64,
    pub opponent_elo: u16,
}

impl MatchSummary {
    pub fn print(&self) {
        println!("games: {}", self.games);
        println!("HI wins: {}", self.hi_wins);
        println!("draws: {}", self.draws);
        println!("HI losses: {}", self.hi_losses);
        println!("score: {:.1}%", self.score * 100.0);
        println!("opponent Elo input: {}", self.opponent_elo);
        println!(
            "HI Elo estimate: {:.1} -> {:.1}",
            self.initial_hi_elo, self.final_hi_elo
        );
    }
}

impl MatchOptions {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self {
            hi_path: env::current_exe().map_err(|error| error.to_string())?,
            opponent_path: PathBuf::new(),
            level: DEFAULT_LEVEL,
            use_model: true,
            model_path: PathBuf::from(DEFAULT_MODEL),
            opponent_options: Vec::new(),
            games: DEFAULT_GAMES,
            time_control: "10+0.1".parse()?,
            hi_elo: DEFAULT_ELO,
            opponent_elo: DEFAULT_ELO,
            openings_path: PathBuf::from(DEFAULT_OPENINGS),
            output_path: PathBuf::from(DEFAULT_OUTPUT),
            max_plies: DEFAULT_MAX_PLIES,
        };
        let mut index = 0;

        while index < arguments.len() {
            match arguments[index].as_str() {
                "--hi" => {
                    options.hi_path =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--hi")?)
                }
                "--opponent" => {
                    options.opponent_path = PathBuf::from(argument_value::<String>(
                        arguments,
                        &mut index,
                        "--opponent",
                    )?)
                }
                "--level" => options.level = argument_value(arguments, &mut index, "--level")?,
                "--model" => {
                    options.model_path =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--model")?)
                }
                "--no-model" => options.use_model = false,
                "--opponent-option" => {
                    let raw = argument_value::<String>(arguments, &mut index, "--opponent-option")?;
                    let (name, value) = raw
                        .split_once('=')
                        .ok_or_else(|| "opponent option must look like Name=Value".to_string())?;
                    if name.trim().is_empty() || value.trim().is_empty() {
                        return Err("opponent option name and value must not be empty".to_string());
                    }
                    options
                        .opponent_options
                        .push((name.trim().to_string(), value.trim().to_string()));
                }
                "--games" => options.games = argument_value(arguments, &mut index, "--games")?,
                "--tc" => {
                    options.time_control =
                        argument_value::<TimeControl>(arguments, &mut index, "--tc")?
                }
                "--hi-elo" => options.hi_elo = argument_value(arguments, &mut index, "--hi-elo")?,
                "--opponent-elo" => {
                    options.opponent_elo = argument_value(arguments, &mut index, "--opponent-elo")?
                }
                "--openings" => {
                    options.openings_path = PathBuf::from(argument_value::<String>(
                        arguments,
                        &mut index,
                        "--openings",
                    )?)
                }
                "--output" => {
                    options.output_path =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--output")?)
                }
                "--max-plies" => {
                    options.max_plies = argument_value(arguments, &mut index, "--max-plies")?
                }
                unknown => return Err(format!("unknown match option: {unknown}")),
            }
            index += 1;
        }

        if options.opponent_path.as_os_str().is_empty() {
            return Err("--opponent is required".to_string());
        }
        if options.games == 0 || options.max_plies == 0 {
            return Err("games and max plies must be positive".to_string());
        }
        if !(1..=5).contains(&options.level) {
            return Err("level must be between 1 and 5".to_string());
        }
        Ok(options)
    }
}

pub fn run_match(options: &MatchOptions) -> Result<MatchSummary, String> {
    let openings = load_openings(&options.openings_path)?;
    let hi_options = vec![
        ("UseModel".to_string(), options.use_model.to_string()),
        (
            "ModelPath".to_string(),
            options.model_path.display().to_string(),
        ),
        ("Level".to_string(), options.level.to_string()),
        ("OpponentElo".to_string(), options.opponent_elo.to_string()),
    ];
    let mut hi = UciEngine::spawn(&options.hi_path, &[], "HI")?;
    hi.handshake(&hi_options)?;
    let mut opponent = UciEngine::spawn(&options.opponent_path, &[], "Opponent")?;
    opponent.handshake(&options.opponent_options)?;

    if let Some(parent) = options
        .output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut output = File::create(&options.output_path).map_err(|error| error.to_string())?;
    writeln!(output, "game\thi_color\tresult\treason\topening\tmoves")
        .map_err(|error| error.to_string())?;

    let mut summary = MatchSummary {
        games: 0,
        hi_wins: 0,
        draws: 0,
        hi_losses: 0,
        score: 0.0,
        initial_hi_elo: options.hi_elo,
        final_hi_elo: f64::from(options.hi_elo),
        opponent_elo: options.opponent_elo,
    };
    let mut rating = f64::from(options.hi_elo);

    for index in 0..options.games {
        hi.new_game()?;
        opponent.new_game()?;
        let opening_index = (index / 2) % openings.len();
        let opening = openings[opening_index].clone();
        let hi_color = if index % 2 == 0 {
            Color::White
        } else {
            Color::Black
        };
        let record = play_game(
            &mut hi,
            &mut opponent,
            GameSettings {
                index,
                opening: &opening,
                hi_color,
                time_control: options.time_control,
                max_plies: options.max_plies,
            },
        )?;
        let score = record.result.hi_score(hi_color);
        match record.result {
            GameResult::WhiteWin | GameResult::BlackWin => {
                if score == 1.0 {
                    summary.hi_wins += 1;
                } else {
                    summary.hi_losses += 1;
                }
            }
            GameResult::Draw => summary.draws += 1,
        }
        let expected = expected_score(rating, f64::from(options.opponent_elo));
        rating += ELO_K_FACTOR * (score - expected);
        summary.games += 1;
        summary.score =
            (summary.hi_wins as f64 + summary.draws as f64 * 0.5) / summary.games as f64;
        summary.final_hi_elo = rating;
        write_record(&mut output, &record)?;
        println!(
            "game {}/{}: HI {} {} ({})",
            index + 1,
            options.games,
            if hi_color == Color::White {
                "White"
            } else {
                "Black"
            },
            record.result.token(),
            record.reason
        );
    }

    Ok(summary)
}

struct GameSettings<'a> {
    index: usize,
    opening: &'a str,
    hi_color: Color,
    time_control: TimeControl,
    max_plies: usize,
}

fn play_game(
    hi: &mut UciEngine,
    opponent: &mut UciEngine,
    settings: GameSettings<'_>,
) -> Result<GameRecord, String> {
    let mut board = Board::from_str(settings.opening)
        .map_err(|error| format!("invalid opening FEN: {error}"))?;
    let mut halfmove_clock = settings
        .opening
        .split_whitespace()
        .nth(4)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let mut hashes = vec![board.get_hash()];
    let mut moves = Vec::new();
    let mut clocks = [settings.time_control.base_ms; 2];

    loop {
        if let Some(result) = board_result(&board, halfmove_clock, &hashes) {
            return Ok(finish_record(settings, result.0, result.1, moves));
        }
        if moves.len() >= settings.max_plies {
            return Ok(finish_record(
                settings,
                GameResult::Draw,
                "move limit".to_string(),
                moves,
            ));
        }

        let side = board.side_to_move();
        let side_index = color_index(side);
        let remaining = clocks[side_index];
        if remaining == 0 {
            let result = if side == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            };
            return Ok(finish_record(
                settings,
                result,
                format!("{} lost on time", color_name(side)),
                moves,
            ));
        }

        let engine = if side == settings.hi_color {
            &mut *hi
        } else {
            &mut *opponent
        };
        let position = position_command(settings.opening, &moves);
        let started = Instant::now();
        let timeout = Duration::from_millis(remaining.saturating_add(ENGINE_GRACE_MS));
        let best_move = match engine.go(
            &position,
            clocks[color_index(Color::White)],
            clocks[color_index(Color::Black)],
            settings.time_control.increment_ms,
            settings.time_control.increment_ms,
            timeout,
        ) {
            Ok(best_move) => best_move,
            Err(error) => {
                let _ = engine.stop();
                let result = if side == Color::White {
                    GameResult::BlackWin
                } else {
                    GameResult::WhiteWin
                };
                return Ok(finish_record(
                    settings,
                    result,
                    format!("{} engine error: {error}", color_name(side)),
                    moves,
                ));
            }
        };
        let elapsed = started.elapsed().as_millis() as u64;
        if elapsed > remaining.saturating_add(CLOCK_FLAG_GRACE_MS) {
            let result = if side == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            };
            return Ok(finish_record(
                settings,
                result,
                format!("{} lost on time", color_name(side)),
                moves,
            ));
        }
        clocks[side_index] = remaining
            .saturating_sub(elapsed)
            .saturating_add(settings.time_control.increment_ms);

        let Some(chess_move) = best_move else {
            let result = if side == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            };
            return Ok(finish_record(
                settings,
                result,
                format!("{} returned no legal move", color_name(side)),
                moves,
            ));
        };
        if !board.legal(chess_move) {
            let result = if side == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            };
            return Ok(finish_record(
                settings,
                result,
                format!("{} returned illegal move {chess_move}", color_name(side)),
                moves,
            ));
        }

        halfmove_clock = halfmove_clock_after(&board, halfmove_clock, chess_move);
        board = board.make_move_new(chess_move);
        hashes.push(board.get_hash());
        moves.push(chess_move);
    }
}

fn finish_record(
    settings: GameSettings<'_>,
    result: GameResult,
    reason: String,
    moves: Vec<ChessMove>,
) -> GameRecord {
    GameRecord {
        index: settings.index,
        hi_color: settings.hi_color,
        result,
        reason,
        opening: settings.opening.to_string(),
        moves,
    }
}

fn board_result(
    board: &Board,
    halfmove_clock: u16,
    hashes: &[u64],
) -> Option<(GameResult, String)> {
    match board.status() {
        BoardStatus::Checkmate => {
            let result = if board.side_to_move() == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            };
            Some((result, "checkmate".to_string()))
        }
        BoardStatus::Stalemate => Some((GameResult::Draw, "stalemate".to_string())),
        BoardStatus::Ongoing => {
            if halfmove_clock >= 100 {
                Some((GameResult::Draw, "fifty-move rule".to_string()))
            } else if hashes
                .iter()
                .filter(|hash| **hash == board.get_hash())
                .count()
                >= 3
            {
                Some((GameResult::Draw, "threefold repetition".to_string()))
            } else if insufficient_material(board) {
                Some((GameResult::Draw, "insufficient material".to_string()))
            } else {
                None
            }
        }
    }
}

fn position_command(opening: &str, moves: &[ChessMove]) -> String {
    let mut command = if opening == START_FEN {
        "startpos".to_string()
    } else {
        format!("fen {opening}")
    };
    if !moves.is_empty() {
        command.push_str(" moves");
        for chess_move in moves {
            command.push(' ');
            command.push_str(&chess_move.to_string());
        }
    }
    command
}

fn load_openings(path: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let content = fs::read_to_string(&path).map_err(|error| {
        format!(
            "failed to read openings {}: {error}",
            path.as_ref().display()
        )
    })?;
    let openings: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if openings.is_empty() {
        return Err("opening file must contain at least one FEN".to_string());
    }
    for opening in &openings {
        let board = Board::from_str(opening)
            .map_err(|error| format!("invalid opening FEN {opening}: {error}"))?;
        let white_kings =
            (*board.pieces(Piece::King) & *board.color_combined(Color::White)).popcnt();
        let black_kings =
            (*board.pieces(Piece::King) & *board.color_combined(Color::Black)).popcnt();
        let piece_count = [
            Piece::Pawn,
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ]
        .into_iter()
        .map(|piece| board.pieces(piece).popcnt())
        .sum::<u32>();
        if white_kings != 1 || black_kings != 1 || piece_count > 32 {
            return Err(format!("invalid opening piece count: {opening}"));
        }
    }
    Ok(openings)
}

fn write_record(output: &mut File, record: &GameRecord) -> Result<(), String> {
    let moves = record
        .moves
        .iter()
        .map(ChessMove::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(
        output,
        "{}\t{}\t{}\t{}\t{}\t{}",
        record.index + 1,
        color_name(record.hi_color),
        record.result.token(),
        record.reason,
        record.opening,
        moves
    )
    .map_err(|error| error.to_string())
}

fn expected_score(rating: f64, opponent_rating: f64) -> f64 {
    1.0 / (1.0 + 10.0_f64.powf((opponent_rating - rating) / 400.0))
}

fn color_index(color: Color) -> usize {
    if color == Color::White { 0 } else { 1 }
}

fn color_name(color: Color) -> &'static str {
    if color == Color::White {
        "White"
    } else {
        "Black"
    }
}

fn argument_value<T: FromStr>(
    arguments: &[String],
    index: &mut usize,
    name: &str,
) -> Result<T, String>
where
    T::Err: Display,
{
    *index += 1;
    let value = arguments
        .get(*index)
        .ok_or_else(|| format!("missing value for {name}"))?;
    value
        .parse()
        .map_err(|error| format!("invalid value for {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_OPENINGS, GameResult, MatchOptions, START_FEN, TimeControl, board_result,
        expected_score, load_openings, position_command,
    };
    use chess::{Board, ChessMove, Square};
    use std::{io::Write, str::FromStr};

    #[test]
    fn parses_time_control() {
        let control = TimeControl::from_str("10+0.1").unwrap();
        assert_eq!(control.base_ms, 10_000);
        assert_eq!(control.increment_ms, 100);
        assert!("0+1".parse::<TimeControl>().is_err());
    }

    #[test]
    fn computes_expected_score() {
        assert!((expected_score(1500.0, 1500.0) - 0.5).abs() < 1e-9);
        assert!(expected_score(1700.0, 1500.0) > 0.5);
    }

    #[test]
    fn builds_position_commands() {
        let chess_move = ChessMove::new(Square::E2, Square::E4, None);
        assert_eq!(
            position_command(START_FEN, &[chess_move]),
            "startpos moves e2e4"
        );
        assert!(position_command("8/8/8/8/8/8/8/K6k w - - 0 1", &[]).starts_with("fen "));
    }

    #[test]
    fn detects_checkmate() {
        let board = Board::from_str("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(
            board_result(&board, 0, &[board.get_hash()]).unwrap().0,
            GameResult::WhiteWin
        );
    }

    #[test]
    fn loads_opening_file() {
        let path = std::env::temp_dir().join(format!("hi-openings-{}.fen", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{START_FEN}").unwrap();
        drop(file);
        let openings = load_openings(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(openings, vec![START_FEN.to_string()]);
    }

    #[test]
    fn rejects_openings_with_extra_pieces() {
        let path =
            std::env::temp_dir().join(format!("hi-openings-invalid-{}.fen", std::process::id()));
        std::fs::write(
            &path,
            "rnbqkb1r/pp3ppp/4pn2/2pp4/2PP4/2N2N2/PP2PPPP/R1BQKBNR w KQkq - 0 1\n",
        )
        .unwrap();
        let result = load_openings(&path);
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn default_openings_are_valid() {
        let openings = load_openings(DEFAULT_OPENINGS).unwrap();
        assert!(openings.len() >= 4);
    }

    #[test]
    fn parses_match_options() {
        let arguments = [
            "--opponent",
            "stockfish",
            "--games",
            "4",
            "--level",
            "5",
            "--no-model",
            "--opponent-option",
            "Skill Level=0",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let options = MatchOptions::parse(&arguments).unwrap();
        assert_eq!(options.opponent_path.to_string_lossy(), "stockfish");
        assert_eq!(options.games, 4);
        assert_eq!(options.level, 5);
        assert!(!options.use_model);
        assert_eq!(options.opponent_options[0].0, "Skill Level");
    }
}
