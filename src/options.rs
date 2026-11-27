use crate::level::{MAX_LEVEL, MIN_LEVEL};
use std::path::PathBuf;

const DEFAULT_MODEL_PATH: &str = "models/hi-v3.bpk";
const DEFAULT_LEVEL: u8 = 3;
const DEFAULT_OPPONENT_ELO: u16 = 1_500;

#[derive(Clone, Debug)]
pub struct UciOptions {
    pub use_model: bool,
    pub model_path: PathBuf,
    pub level: u8,
    pub opponent_elo: u16,
}

impl Default for UciOptions {
    fn default() -> Self {
        Self {
            use_model: true,
            model_path: PathBuf::from(DEFAULT_MODEL_PATH),
            level: DEFAULT_LEVEL,
            opponent_elo: DEFAULT_OPPONENT_ELO,
        }
    }
}

impl UciOptions {
    pub fn apply_setoption(&mut self, tokens: &[&str]) -> Result<(), String> {
        let name = tokens
            .get(2)
            .copied()
            .ok_or_else(|| "setoption requires a name".to_string())?;
        let value_index = tokens
            .iter()
            .position(|token| *token == "value")
            .ok_or_else(|| "setoption requires a value".to_string())?;
        let value = tokens[value_index + 1..].join(" ");

        match name {
            "UseModel" => self.use_model = parse_bool(&value)?,
            "ModelPath" => {
                if value.trim().is_empty() {
                    return Err("ModelPath must not be empty".to_string());
                }
                self.model_path = PathBuf::from(value);
            }
            "Level" => {
                let level = value
                    .parse::<u8>()
                    .map_err(|error| format!("invalid Level: {error}"))?;
                if !(MIN_LEVEL..=MAX_LEVEL).contains(&level) {
                    return Err(format!("Level must be between {MIN_LEVEL} and {MAX_LEVEL}"));
                }
                self.level = level;
            }
            "OpponentElo" => {
                let elo = value
                    .parse::<u16>()
                    .map_err(|error| format!("invalid OpponentElo: {error}"))?;
                if !(600..=2_200).contains(&elo) {
                    return Err("OpponentElo must be between 600 and 2200".to_string());
                }
                self.opponent_elo = elo;
            }
            _ => return Err(format!("unsupported option: {name}")),
        }
        Ok(())
    }

    pub fn declaration_lines(&self) -> Vec<String> {
        vec![
            format!("option name UseModel type check default {}", self.use_model),
            format!(
                "option name ModelPath type string default {}",
                self.model_path.display()
            ),
            format!(
                "option name Level type spin default {} min {MIN_LEVEL} max {MAX_LEVEL}",
                self.level
            ),
            format!(
                "option name OpponentElo type spin default {} min 600 max 2200",
                self.opponent_elo
            ),
        ]
    }
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::UciOptions;

    #[test]
    fn applies_supported_options() {
        let mut options = UciOptions::default();
        options
            .apply_setoption(&["setoption", "name", "Level", "value", "5"])
            .unwrap();
        options
            .apply_setoption(&[
                "setoption",
                "name",
                "ModelPath",
                "value",
                "models/custom.bpk",
            ])
            .unwrap();
        options
            .apply_setoption(&["setoption", "name", "UseModel", "value", "false"])
            .unwrap();
        assert_eq!(options.level, 5);
        assert_eq!(options.model_path.to_string_lossy(), "models/custom.bpk");
        assert!(!options.use_model);
    }

    #[test]
    fn rejects_invalid_level() {
        let mut options = UciOptions::default();
        assert!(
            options
                .apply_setoption(&["setoption", "name", "Level", "value", "9"])
                .is_err()
        );
    }

    #[test]
    fn declares_four_options() {
        let options = UciOptions::default();
        assert_eq!(options.declaration_lines().len(), 4);
    }
}
