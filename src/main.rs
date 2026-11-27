use chess::Board;
use hi_chess::{arena, bench, features::top_policy_moves, inference, perft, prepare, train, uci};
use std::{env, fmt::Display, path::PathBuf, process, str::FromStr};

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();

    match arguments.as_slice() {
        [] => uci::run(),
        [command, rest @ ..] if command == "perft" => run_perft(rest),
        [command, rest @ ..] if command == "prepare" => run_prepare(rest),
        [command, rest @ ..] if command == "train" => run_train(rest),
        [command, rest @ ..] if command == "predict" => run_predict(rest),
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
    match prepare::prepare_compressed(
        &options.input,
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
            model: PathBuf::from("models/hi-v1.bpk"),
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
        "  hi prepare --input <games.pgn.zst> [--train <train.bin>] [--validation <validation.bin>]"
    );
    println!(
        "  hi train [--train <train.bin>] [--validation <validation.bin>] [--output <model.bpk>]"
    );
    println!("  hi predict [--model <model.bpk>] [--fen <fen>] [--elo <rating>]");
    println!("  hi match --opponent <engine> [--level <1-5>] [--games <n>] [--tc <10+0.1>]");
    println!("  hi bench [--depth <n>]");
    println!("  hi --help");
}

fn fail(message: &str) -> ! {
    eprintln!("HI: {message}");
    process::exit(2);
}
