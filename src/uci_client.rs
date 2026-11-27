use chess::ChessMove;
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const READY_TIMEOUT: Duration = Duration::from_secs(60);

enum OutputEvent {
    Line(String),
    Closed,
}

pub struct UciEngine {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    output: Receiver<OutputEvent>,
    name: String,
}

impl UciEngine {
    pub fn spawn(path: impl AsRef<Path>, arguments: &[String], name: &str) -> Result<Self, String> {
        let mut child = Command::new(path.as_ref())
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("failed to start {name}: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{name} did not provide stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("{name} did not provide stdout"))?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || read_output(stdout, sender));

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            output: receiver,
            name: name.to_string(),
        })
    }

    pub fn handshake(&mut self, options: &[(String, String)]) -> Result<(), String> {
        self.send("uci")?;
        self.wait_for_line("uciok", HANDSHAKE_TIMEOUT)?;
        for (name, value) in options {
            self.setoption(name, value)?;
        }
        self.send("isready")?;
        self.wait_for_line("readyok", READY_TIMEOUT)?;
        Ok(())
    }

    pub fn new_game(&mut self) -> Result<(), String> {
        self.send("ucinewgame")?;
        self.send("isready")?;
        self.wait_for_line("readyok", READY_TIMEOUT)?;
        Ok(())
    }

    pub fn go(
        &mut self,
        position: &str,
        white_time_ms: u64,
        black_time_ms: u64,
        white_increment_ms: u64,
        black_increment_ms: u64,
        timeout: Duration,
    ) -> Result<Option<ChessMove>, String> {
        self.send(&format!("position {position}"))?;
        self.send(&format!(
            "go wtime {white_time_ms} btime {black_time_ms} winc {white_increment_ms} binc {black_increment_ms}"
        ))?;
        let line = self.wait_for_line("bestmove", timeout)?;
        parse_bestmove(&line)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.send("stop")
    }

    pub fn close(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn setoption(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.send(&format!("setoption name {name} value {value}"))
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.stdin, "{line}")
            .map_err(|error| format!("failed to write to {}: {error}", self.name))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush {}: {error}", self.name))
    }

    fn wait_for_line(&mut self, prefix: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(OutputEvent::Line(line)) if line.starts_with(prefix) => return Ok(line),
                Ok(OutputEvent::Line(_)) => {}
                Ok(OutputEvent::Closed) => {
                    return Err(format!(
                        "{} closed stdout while waiting for {prefix}",
                        self.name
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("{} timed out waiting for {prefix}", self.name));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!(
                        "{} disconnected while waiting for {prefix}",
                        self.name
                    ));
                }
            }
        }
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        self.close();
    }
}

fn read_output(stdout: ChildStdout, sender: Sender<OutputEvent>) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                if sender.send(OutputEvent::Line(line)).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = sender.send(OutputEvent::Closed);
}

fn parse_bestmove(line: &str) -> Result<Option<ChessMove>, String> {
    let move_text = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("invalid bestmove line: {line}"))?;
    if move_text == "(none)" {
        return Ok(None);
    }
    move_text
        .parse::<ChessMove>()
        .map(Some)
        .map_err(|error| format!("invalid bestmove {move_text}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::parse_bestmove;
    use chess::{ChessMove, Piece, Square};

    #[test]
    fn parses_regular_move() {
        assert_eq!(
            parse_bestmove("bestmove e2e4").unwrap(),
            Some(ChessMove::new(Square::E2, Square::E4, None))
        );
    }

    #[test]
    fn parses_promotion() {
        assert_eq!(
            parse_bestmove("bestmove a7a8q ponder a2a3").unwrap(),
            Some(ChessMove::new(Square::A7, Square::A8, Some(Piece::Queen)))
        );
    }

    #[test]
    fn parses_no_move() {
        assert_eq!(parse_bestmove("bestmove (none)").unwrap(), None);
    }

    #[test]
    fn rejects_invalid_bestmove() {
        assert!(parse_bestmove("bestmove e2e9").is_err());
    }
}
