use crate::{
    dataset::{TrainingRecord, write_records},
    search::halfmove_clock_after,
};
use chess::{Board, ChessMove, Color};
use pgn_reader::{KnownOutcome, Outcome, RawTag, Reader, SanPlus, Visitor};
use std::{
    fs::File,
    io::{self, Read},
    ops::ControlFlow,
    path::{Path, PathBuf},
};

const MIN_SKIP_PLIES: usize = 10;
const MAX_POSITIONS_PER_GAME: usize = 24;
const VALIDATION_INTERVAL: usize = 10;
const MIN_ELO: u16 = 600;
const MAX_ELO: u16 = 2_200;

#[derive(Clone, Debug, Default)]
pub struct PrepareStats {
    pub games_scanned: usize,
    pub games_used: usize,
    pub games_skipped: usize,
    pub train_records: usize,
    pub validation_records: usize,
}

#[derive(Default)]
struct GameData {
    fen: Option<String>,
    white_elo: Option<String>,
    black_elo: Option<String>,
    event: Option<String>,
    result_tag: Option<String>,
    outcome: Option<Outcome>,
    moves: Vec<SanPlus>,
}

struct GameVisitor;

impl Visitor for GameVisitor {
    type Tags = GameData;
    type Movetext = GameData;
    type Output = io::Result<GameData>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(GameData::default())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(tags)
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let text = match value.decode_utf8() {
            Ok(text) => text.into_owned(),
            Err(error) => {
                return ControlFlow::Break(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid PGN tag: {error}"),
                )));
            }
        };

        match name {
            b"FEN" => tags.fen = Some(text),
            b"WhiteElo" => tags.white_elo = Some(text),
            b"BlackElo" => tags.black_elo = Some(text),
            b"Event" => tags.event = Some(text),
            b"Result" => tags.result_tag = Some(text),
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn san(
        &mut self,
        movetext: &mut Self::Movetext,
        san_plus: SanPlus,
    ) -> ControlFlow<Self::Output> {
        movetext.moves.push(san_plus);
        ControlFlow::Continue(())
    }

    fn outcome(
        &mut self,
        movetext: &mut Self::Movetext,
        outcome: Outcome,
    ) -> ControlFlow<Self::Output> {
        movetext.outcome = Some(outcome);
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, movetext: Self::Movetext) -> Self::Output {
        Ok(movetext)
    }
}

struct DatasetBuffer {
    train_records: Vec<TrainingRecord>,
    validation_records: Vec<TrainingRecord>,
    stats: PrepareStats,
}

impl DatasetBuffer {
    fn new(max_train: usize, max_validation: usize) -> Self {
        Self {
            train_records: Vec::with_capacity(max_train),
            validation_records: Vec::with_capacity(max_validation),
            stats: PrepareStats::default(),
        }
    }

    fn is_full(&self, max_train: usize, max_validation: usize) -> bool {
        self.train_records.len() >= max_train && self.validation_records.len() >= max_validation
    }

    fn read(
        &mut self,
        mut input: impl Read,
        max_train: usize,
        max_validation: usize,
    ) -> io::Result<()> {
        let mut reader = Reader::new(&mut input);
        let mut visitor = GameVisitor;

        while !self.is_full(max_train, max_validation) {
            let Some(game_result) = reader.read_game(&mut visitor)? else {
                break;
            };
            self.stats.games_scanned += 1;
            let game = match game_result {
                Ok(game) => game,
                Err(_) => {
                    self.stats.games_skipped += 1;
                    continue;
                }
            };
            let validation = self.stats.games_scanned.is_multiple_of(VALIDATION_INTERVAL);
            let train_capacity = max_train.saturating_sub(self.train_records.len());
            let validation_capacity = max_validation.saturating_sub(self.validation_records.len());
            let capacity = if validation {
                validation_capacity
            } else {
                train_capacity
            };
            let records = match build_game_records(game, capacity) {
                Ok(Some(records)) if !records.is_empty() => records,
                Ok(_) => {
                    self.stats.games_skipped += 1;
                    continue;
                }
                Err(_) => {
                    self.stats.games_skipped += 1;
                    continue;
                }
            };

            if validation {
                self.validation_records.extend(records);
            } else {
                self.train_records.extend(records);
            }
            self.stats.games_used += 1;
        }
        Ok(())
    }

    fn write(
        mut self,
        train_path: impl AsRef<Path>,
        validation_path: impl AsRef<Path>,
    ) -> io::Result<PrepareStats> {
        self.stats.train_records = self.train_records.len();
        self.stats.validation_records = self.validation_records.len();
        write_records(train_path, &self.train_records)?;
        write_records(validation_path, &self.validation_records)?;
        Ok(self.stats)
    }
}

pub fn prepare(
    input: impl Read,
    train_path: impl AsRef<Path>,
    validation_path: impl AsRef<Path>,
    max_train: usize,
    max_validation: usize,
) -> io::Result<PrepareStats> {
    let mut buffer = DatasetBuffer::new(max_train, max_validation);
    buffer.read(input, max_train, max_validation)?;
    buffer.write(train_path, validation_path)
}

pub fn prepare_compressed(
    input_path: impl AsRef<Path>,
    train_path: impl AsRef<Path>,
    validation_path: impl AsRef<Path>,
    max_train: usize,
    max_validation: usize,
) -> io::Result<PrepareStats> {
    prepare_compressed_files(
        std::slice::from_ref(&input_path),
        train_path,
        validation_path,
        max_train,
        max_validation,
    )
}

pub fn prepare_compressed_files<P: AsRef<Path>>(
    input_paths: &[P],
    train_path: impl AsRef<Path>,
    validation_path: impl AsRef<Path>,
    max_train: usize,
    max_validation: usize,
) -> io::Result<PrepareStats> {
    if input_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "at least one input is required",
        ));
    }
    let mut buffer = DatasetBuffer::new(max_train, max_validation);
    for (index, input_path) in input_paths.iter().enumerate() {
        if buffer.is_full(max_train, max_validation) {
            break;
        }
        let remaining_inputs = input_paths.len() - index;
        let train_target = buffer.train_records.len()
            + max_train.saturating_sub(buffer.train_records.len()) / remaining_inputs;
        let validation_target = buffer.validation_records.len()
            + max_validation.saturating_sub(buffer.validation_records.len()) / remaining_inputs;
        let file = File::open(input_path)?;
        let decoder = zstd::stream::read::Decoder::new(file)?;
        buffer.read(decoder, train_target, validation_target)?;
    }
    buffer.write(train_path, validation_path)
}

#[derive(Clone, Debug)]
pub struct PrepareOptions {
    pub inputs: Vec<PathBuf>,
    pub train: PathBuf,
    pub validation: PathBuf,
    pub max_train: usize,
    pub max_validation: usize,
}

impl PrepareOptions {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut inputs = Vec::new();
        let mut train = PathBuf::from("data/train.bin");
        let mut validation = PathBuf::from("data/validation.bin");
        let mut max_train = 240_000;
        let mut max_validation = 24_000;
        let mut index = 0;

        while index < arguments.len() {
            match arguments[index].as_str() {
                "--input" => inputs.push(PathBuf::from(argument_value::<String>(
                    arguments, &mut index, "--input",
                )?)),
                "--train" => {
                    train =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--train")?)
                }
                "--validation" => {
                    validation = PathBuf::from(argument_value::<String>(
                        arguments,
                        &mut index,
                        "--validation",
                    )?)
                }
                "--max-train" => max_train = argument_value(arguments, &mut index, "--max-train")?,
                "--max-validation" => {
                    max_validation = argument_value(arguments, &mut index, "--max-validation")?
                }
                unknown => return Err(format!("unknown prepare option: {unknown}")),
            }
            index += 1;
        }

        if inputs.is_empty() {
            return Err("--input is required".to_string());
        }
        if max_train == 0 || max_validation == 0 {
            return Err("dataset limits must be positive".to_string());
        }
        Ok(Self {
            inputs,
            train,
            validation,
            max_train,
            max_validation,
        })
    }
}

fn argument_value<T: std::str::FromStr>(
    arguments: &[String],
    index: &mut usize,
    name: &str,
) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    *index += 1;
    let value = arguments
        .get(*index)
        .ok_or_else(|| format!("missing value for {name}"))?;
    value
        .parse()
        .map_err(|error| format!("invalid value for {name}: {error}"))
}

fn build_game_records(
    game: GameData,
    capacity: usize,
) -> Result<Option<Vec<TrainingRecord>>, String> {
    if !game
        .event
        .as_deref()
        .is_some_and(|event| event.contains("Rated"))
    {
        return Ok(None);
    }
    let white_elo = parse_rating(game.white_elo.as_deref())?;
    let black_elo = parse_rating(game.black_elo.as_deref())?;
    let (Some(white_elo), Some(black_elo)) = (white_elo, black_elo) else {
        return Ok(None);
    };
    if game.moves.len() <= MIN_SKIP_PLIES {
        return Ok(None);
    }

    let outcome = known_outcome(&game)?;
    let mut board = match game.fen.as_deref() {
        Some(fen) => fen
            .parse::<Board>()
            .map_err(|error| format!("invalid FEN: {error}"))?,
        None => Board::default(),
    };
    let mut halfmove_clock = game.fen.as_deref().map(initial_halfmove_clock).unwrap_or(0);
    let mut last_move = None;
    let capacity = capacity.min(MAX_POSITIONS_PER_GAME);
    if capacity == 0 {
        return Ok(None);
    }
    let mut records = Vec::with_capacity(capacity);

    for (ply, san) in game.moves.into_iter().enumerate() {
        let san_text = san.to_string();
        let chess_move = ChessMove::from_san(&board, &san_text)
            .map_err(|error| format!("invalid SAN {san_text}: {error}"))?;

        if ply >= MIN_SKIP_PLIES && records.len() < capacity {
            let (own_elo, opponent_elo) = if board.side_to_move() == Color::White {
                (white_elo, black_elo)
            } else {
                (black_elo, white_elo)
            };
            let result = result_for_side(outcome, board.side_to_move());
            records.push(
                TrainingRecord::new(
                    &board,
                    own_elo,
                    opponent_elo,
                    halfmove_clock,
                    last_move,
                    chess_move,
                    result,
                )
                .map_err(|error| error.to_string())?,
            );
        }

        halfmove_clock = halfmove_clock_after(&board, halfmove_clock, chess_move);
        board = board.make_move_new(chess_move);
        last_move = Some(chess_move);
    }

    records.truncate(capacity);
    Ok(Some(records))
}

fn parse_rating(value: Option<&str>) -> Result<Option<u16>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let rating = value
        .trim()
        .parse::<u16>()
        .map_err(|error| format!("invalid rating: {error}"))?;
    if !(MIN_ELO..=MAX_ELO).contains(&rating) {
        return Ok(None);
    }
    Ok(Some(rating))
}

fn known_outcome(game: &GameData) -> Result<KnownOutcome, String> {
    if let Some(outcome) = game.outcome.and_then(Outcome::known) {
        return Ok(outcome);
    }
    let raw = game
        .result_tag
        .as_deref()
        .ok_or_else(|| "missing result".to_string())?;
    let outcome = Outcome::from_ascii(raw.trim().as_bytes())
        .map_err(|error| format!("invalid result: {error}"))?;
    outcome.known().ok_or_else(|| "unknown result".to_string())
}

fn result_for_side(outcome: KnownOutcome, side: Color) -> u8 {
    match outcome {
        KnownOutcome::Draw => 1,
        KnownOutcome::Decisive { winner } => {
            let winner = if winner == pgn_reader::shakmaty::Color::White {
                Color::White
            } else {
                Color::Black
            };
            if side == winner { 2 } else { 0 }
        }
    }
}

fn initial_halfmove_clock(fen: &str) -> u16 {
    fen.split_whitespace()
        .nth(4)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{PrepareOptions, prepare, prepare_compressed_files};
    use std::io::Cursor;

    const SAMPLE_PGN: &str = r#"[Event "Rated Rapid game"]
[Site "https://lichess.org/test"]
[Date "2014.12.01"]
[White "Test White"]
[Black "Test Black"]
[Result "1-0"]
[WhiteElo "1500"]
[BlackElo "1600"]
[TimeControl "600+0"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5
7. Bb3 d6 8. c3 O-O 1-0

[Event "Rated Rapid game"]
[Site "https://lichess.org/test2"]
[Date "2014.12.01"]
[White "Test White"]
[Black "Test Black"]
[Result "0-1"]
[WhiteElo "1400"]
[BlackElo "1700"]
[TimeControl "600+0"]

1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 5. e3 O-O 6. Nf3 h6
7. Bh4 b6 8. Bd3 Bb7 0-1
"#;

    #[test]
    fn prepares_records_from_pgn() {
        let path = std::env::temp_dir().join(format!(
            "hi-prepare-{}-{}.bin",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let train_path = path.with_extension("train.bin");
        let validation_path = path.with_extension("validation.bin");
        let stats = prepare(
            Cursor::new(SAMPLE_PGN),
            &train_path,
            &validation_path,
            100,
            100,
        )
        .unwrap();
        let train = crate::dataset::read_records(&train_path).unwrap();
        let validation = crate::dataset::read_records(&validation_path).unwrap();
        std::fs::remove_file(train_path).unwrap();
        std::fs::remove_file(validation_path).unwrap();

        assert_eq!(stats.games_scanned, 2);
        assert_eq!(stats.games_used, 2);
        assert_eq!(stats.games_skipped, 0);
        assert_eq!(stats.train_records, train.len());
        assert_eq!(stats.validation_records, validation.len());
        assert_eq!(train.len() + validation.len(), 12);
    }

    #[test]
    fn parses_multiple_inputs() {
        let arguments = ["--input", "one.zst", "--input", "two.zst"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let options = PrepareOptions::parse(&arguments).unwrap();
        assert_eq!(options.inputs.len(), 2);
        assert_eq!(options.inputs[1].to_string_lossy(), "two.zst");
    }

    #[test]
    fn shares_the_validation_split_across_files() {
        let stem = format!("hi-prepare-multi-{}", std::process::id());
        let first = std::env::temp_dir().join(format!("{stem}-first.zst"));
        let second = std::env::temp_dir().join(format!("{stem}-second.zst"));
        let repeated_games = SAMPLE_PGN.repeat(5);
        for path in [&first, &second] {
            let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 0).unwrap();
            std::io::Write::write_all(&mut encoder, repeated_games.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();
            std::fs::write(path, compressed).unwrap();
        }
        let train_path = std::env::temp_dir().join(format!("{stem}-train.bin"));
        let validation_path = std::env::temp_dir().join(format!("{stem}-validation.bin"));
        let stats = prepare_compressed_files(
            &[first.clone(), second.clone()],
            &train_path,
            &validation_path,
            54,
            12,
        )
        .unwrap();
        let train = crate::dataset::read_records(&train_path).unwrap();
        let validation = crate::dataset::read_records(&validation_path).unwrap();
        for path in [&first, &second, &train_path, &validation_path] {
            std::fs::remove_file(path).unwrap();
        }

        assert_eq!(stats.train_records, 54);
        assert_eq!(stats.validation_records, 12);
        assert_eq!(train.len(), 54);
        assert_eq!(validation.len(), 12);
    }
}
