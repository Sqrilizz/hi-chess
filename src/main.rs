use chess::{Board, ChessMove, Color};
use hi_chess::{
    arena, bench, features::top_policy_moves, inference, perft, personality, prepare, review,
    train, uci,
};
use std::{env, fmt::Display, path::PathBuf, process, str::FromStr};

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();

    match arguments.as_slice() {
        [] => uci::run(),
        [command, rest @ ..] if command == "perft" => run_perft(rest),
        [command, rest @ ..] if command == "prepare" => run_prepare(rest),
        [command, rest @ ..] if command == "train" => run_train(rest),
        [command, rest @ ..] if command == "evaluate" => run_evaluate(rest),
        [command, rest @ ..] if command == "predict" => run_predict(rest),
        [command, rest @ ..] if command == "review" => run_review(rest),
        [command, rest @ ..] if command == "say" => run_say(rest),
        [command, rest @ ..] if command == "match" => run_match(rest),
        [command, rest @ ..] if command == "bench" => run_bench(rest),
        [flag] if flag == "--help" || flag == "-h" => print_usage(),
        _ => fail("invalid arguments"),
    }
}

fn run_perft(arguments: &[String]) {
    if arguments.len() != 2 {
        fail("usage: hi perft <fen> <depth>");
    }
    let depth = match arguments[1].parse::<usize>() {
        Ok(depth) => depth,
        Err(_) => fail("depth must be a non-negative integer"),
    };
    match Board::from_str(&arguments[0]) {
        Ok(board) => println!("{}", perft::perft(&board, depth)),
        Err(error) => fail(&format!("invalid FEN: {error}")),
    }
}

fn run_prepare(arguments: &[String]) {
    let options = match prepare::PrepareOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    match prepare::prepare_compressed_files(
        &options.inputs,
        &options.train,
        &options.validation,
        options.max_train,
        options.max_validation,
    ) {
        Ok(stats) => {
            println!("games scanned: {}", stats.games_scanned);
            println!("games used: {}", stats.games_used);
            println!("games skipped: {}", stats.games_skipped);
            println!("train records: {}", stats.train_records);
            println!("validation records: {}", stats.validation_records);
        }
        Err(error) => fail(&format!("dataset preparation failed: {error}")),
    }
}

fn run_train(arguments: &[String]) {
    let options = match train::TrainOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    match train::train(&options) {
        Ok(stats) => {
            println!("steps: {}", stats.steps);
            println!("best epoch: {}", stats.best_epoch);
            println!(
                "best validation top1: {:.2}%",
                stats.best_validation_top1 * 100.0
            );
            println!("final train loss: {:.4}", stats.final_loss);
            println!("model: {}", options.output.display());
        }
        Err(error) => fail(&format!("training failed: {error}")),
    }
}

fn run_evaluate(arguments: &[String]) {
    let options = match EvaluateOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    let records = match hi_chess::dataset::read_records(&options.validation) {
        Ok(records) => records,
        Err(error) => fail(&format!("validation read failed: {error}")),
    };
    let device = train::device_for(options.device);
    match train::evaluate_checkpoint(&options.model, &records, &device, options.batch_size) {
        Ok(metrics) => {
            println!("records: {}", metrics.samples);
            println!("loss: {:.4}", metrics.loss);
            println!("top1: {:.2}%", metrics.top1 * 100.0);
            println!("top3: {:.2}%", metrics.top3 * 100.0);
            println!("wdl: {:.2}%", metrics.value_accuracy * 100.0);
            println!("white top1: {:.2}%", metrics.white_top1 * 100.0);
            println!("black top1: {:.2}%", metrics.black_top1 * 100.0);
        }
        Err(error) => fail(&format!("evaluation failed: {error}")),
    }
}

fn run_predict(arguments: &[String]) {
    let options = match PredictOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    let board = match Board::from_str(&options.fen) {
        Ok(board) => board,
        Err(error) => fail(&format!("invalid FEN: {error}")),
    };
    let runner = match inference::ModelRunner::new(&options.model, options.discrete_gpu) {
        Ok(runner) => runner,
        Err(error) => fail(&format!("model load failed: {error}")),
    };
    let halfmove_clock = options
        .fen
        .split_whitespace()
        .nth(4)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let policy = match runner.predict(
        &board,
        options.elo,
        options.opponent_elo,
        halfmove_clock,
        None,
        options.temperature,
    ) {
        Ok(policy) => policy,
        Err(error) => fail(&format!("inference failed: {error}")),
    };

    println!(
        "WDL: {:.1}% / {:.1}% / {:.1}%",
        policy.wdl[0] * 100.0,
        policy.wdl[1] * 100.0,
        policy.wdl[2] * 100.0
    );
    for (index, policy_move) in top_policy_moves(&policy, 10).iter().enumerate() {
        println!(
            "{}. {} {:.2}%",
            index + 1,
            policy_move.chess_move,
            policy_move.probability * 100.0
        );
    }
}

fn run_bench(arguments: &[String]) {
    let options = match bench::BenchOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    if let Err(error) = bench::run(&options) {
        fail(&format!("benchmark failed: {error}"));
    }
}

fn run_say(arguments: &[String]) {
    let options = match SayOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    println!("{}", personality::line(options.moment, options.variation));
}

fn run_review(arguments: &[String]) {
    let options = match ReviewOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    match review::review_game(&options.fen, &options.moves, options.depth, options.player) {
        Ok(result) => match serde_json::to_string_pretty(&result) {
            Ok(json) => println!("{json}"),
            Err(error) => fail(&format!("review serialization failed: {error}")),
        },
        Err(error) => fail(&format!("review failed: {error}")),
    }
}

fn run_match(arguments: &[String]) {
    let options = match arena::MatchOptions::parse(arguments) {
        Ok(options) => options,
        Err(error) => fail(&error),
    };
    match arena::run_match(&options) {
        Ok(summary) => {
            summary.print();
            println!("match log: {}", options.output_path.display());
        }
        Err(error) => fail(&format!("match failed: {error}")),
    }
}

struct SayOptions {
    moment: personality::Moment,
    variation: usize,
}

impl SayOptions {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut moment = None;
        let mut variation = 0;
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].as_str() {
                "--moment" => {
                    let value = argument_value::<String>(arguments, &mut index, "--moment")?;
                    moment = Some(value.parse()?);
                }
                "--variation" => variation = argument_value(arguments, &mut index, "--variation")?,
                unknown => return Err(format!("unknown say option: {unknown}")),
            }
            index += 1;
        }
        let moment = moment.ok_or_else(|| "--moment is required".to_string())?;
        Ok(Self { moment, variation })
    }
}

struct EvaluateOptions {
    model: PathBuf,
    validation: PathBuf,
    batch_size: usize,
    device: train::DeviceSelection,
}

impl EvaluateOptions {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self {
            model: PathBuf::from("models/hi-v3.bpk"),
            validation: PathBuf::from("data/validation-v3.bin"),
            batch_size: 128,
            device: train::DeviceSelection::Auto,
        };
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].as_str() {
                "--model" => {
                    options.model =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--model")?)
                }
                "--validation" => {
                    options.validation = PathBuf::from(argument_value::<String>(
                        arguments,
                        &mut index,
                        "--validation",
                    )?)
                }
                "--batch-size" => {
                    options.batch_size = argument_value(arguments, &mut index, "--batch-size")?
                }
                "--device" => {
                    let value = argument_value::<String>(arguments, &mut index, "--device")?;
                    options.device = match value.as_str() {
                        "auto" => train::DeviceSelection::Auto,
                        "discrete" => train::DeviceSelection::Discrete,
                        "cpu" => train::DeviceSelection::Cpu,
                        _ => return Err("device must be auto, discrete or cpu".to_string()),
                    };
                }
                unknown => return Err(format!("unknown evaluate option: {unknown}")),
            }
            index += 1;
        }
        if options.batch_size == 0 {
            return Err("batch size must be positive".to_string());
        }
        Ok(options)
    }
}

struct ReviewOptions {
    fen: String,
    moves: Vec<ChessMove>,
    depth: u16,
    player: Color,
}

impl ReviewOptions {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut fen = START_FEN.to_string();
        let mut move_text = None;
        let mut depth = review::default_depth();
        let mut player = Color::White;
        let mut index = 0;

        while index < arguments.len() {
            match arguments[index].as_str() {
                "--fen" => fen = argument_value(arguments, &mut index, "--fen")?,
                "--moves" => {
                    move_text = Some(argument_value::<String>(arguments, &mut index, "--moves")?)
                }
                "--depth" => depth = argument_value(arguments, &mut index, "--depth")?,
                "--player" => {
                    let value = argument_value::<String>(arguments, &mut index, "--player")?;
                    player = match value.as_str() {
                        "white" => Color::White,
                        "black" => Color::Black,
                        _ => return Err("player must be white or black".to_string()),
                    };
                }
                unknown => return Err(format!("unknown review option: {unknown}")),
            }
            index += 1;
        }

        let move_text = move_text.ok_or_else(|| "--moves is required".to_string())?;
        let moves: Vec<ChessMove> = move_text
            .split_whitespace()
            .map(ChessMove::from_str)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("invalid move: {error}"))?;
        if moves.is_empty() {
            return Err("--moves must contain at least one move".to_string());
        }
        if depth == 0 || depth > review::max_depth() {
            return Err(format!(
                "depth must be between 1 and {}",
                review::max_depth()
            ));
        }
        Ok(Self {
            fen,
            moves,
            depth,
            player,
        })
    }
}

struct PredictOptions {
    model: PathBuf,
    fen: String,
    elo: u16,
    opponent_elo: u16,
    temperature: f32,
    discrete_gpu: bool,
}

impl PredictOptions {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self {
            model: PathBuf::from("models/hi-v3.bpk"),
            fen: START_FEN.to_string(),
            elo: 1_500,
            opponent_elo: 1_500,
            temperature: 1.0,
            discrete_gpu: false,
        };
        let mut index = 0;

        while index < arguments.len() {
            match arguments[index].as_str() {
                "--model" => {
                    options.model =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--model")?)
                }
                "--fen" => options.fen = argument_value(arguments, &mut index, "--fen")?,
                "--elo" => options.elo = argument_value(arguments, &mut index, "--elo")?,
                "--opponent-elo" => {
                    options.opponent_elo = argument_value(arguments, &mut index, "--opponent-elo")?
                }
                "--temperature" => {
                    options.temperature = argument_value(arguments, &mut index, "--temperature")?
                }
                "--device" => {
                    let device = argument_value::<String>(arguments, &mut index, "--device")?;
                    options.discrete_gpu = match device.as_str() {
                        "auto" => false,
                        "discrete" => true,
                        _ => return Err("device must be auto or discrete".to_string()),
                    };
                }
                unknown => return Err(format!("unknown predict option: {unknown}")),
            }
            index += 1;
        }

        if !options.temperature.is_finite() || options.temperature <= 0.0 {
            return Err("temperature must be positive and finite".to_string());
        }
        Ok(options)
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

fn print_usage() {
    println!("HI chess engine");
    println!();
    println!("Usage:");
    println!("  hi");
    println!("  hi perft <fen> <depth>");
    println!(
        "  hi prepare --input <games.pgn.zst> [--input <more.pgn.zst> ...] [--train <train.bin>] [--validation <validation.bin>]"
    );
    println!(
        "  hi train [--train <train.bin>] [--validation <validation.bin>] [--output <model.bpk>] [--resume <model.bpk>]"
    );
    println!(
        "  hi evaluate [--model <model.bpk>] [--validation <validation.bin>] [--device <auto|discrete|cpu>]"
    );
    println!("  hi predict [--model <model.bpk>] [--fen <fen>] [--elo <rating>]");
    println!(
        "  hi review --moves <uci moves> [--fen <fen>] [--depth <n>] [--player <white|black>]"
    );
    println!("  hi say --moment <moment> [--variation <n>]");
    println!("  hi match --opponent <engine> [--level <1-5>] [--games <n>] [--tc <10+0.1>]");
    println!("  hi bench [--depth <n>]");
    println!("  hi --help");
}

fn fail(message: &str) -> ! {
    eprintln!("HI: {message}");
    process::exit(2);
}
