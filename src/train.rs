use crate::{
    dataset::{TrainingRecord, read_records},
    features::{INPUT_CHANNELS, POLICY_MOVES, VALUE_CLASSES},
    network::{ChessNet, training_device},
};
use burn::{
    nn::loss::CrossEntropyLossConfig,
    optim::{AdamConfig, GradientsParams, ModuleOptimizer},
    prelude::*,
    tensor::DeviceKind,
};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::{fmt::Display, fs, ops::Not, path::PathBuf, time::Instant};

const VALUE_LOSS_WEIGHT: f32 = 0.25;
const LOG_INTERVAL: usize = 25;
const LR_DECAY_EPOCHS: u32 = 5;
const LR_DECAY_FACTOR: f32 = 0.85;

#[derive(Clone, Debug)]
pub struct TrainOptions {
    pub train: PathBuf,
    pub validation: PathBuf,
    pub output: PathBuf,
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f64,
    pub seed: u64,
    pub device: DeviceSelection,
    pub max_steps: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSelection {
    Auto,
    Discrete,
    Cpu,
}

#[derive(Clone, Debug, Default)]
pub struct TrainStats {
    pub steps: usize,
    pub best_epoch: usize,
    pub best_validation_top1: f32,
    pub final_loss: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    loss: f32,
    top1: f32,
    top3: f32,
    value_accuracy: f32,
    samples: usize,
}

impl TrainOptions {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self {
            train: PathBuf::from("data/train.bin"),
            validation: PathBuf::from("data/validation.bin"),
            output: PathBuf::from("models/hi-v1.bpk"),
            epochs: 10,
            batch_size: 128,
            learning_rate: 1e-3,
            seed: 42,
            device: DeviceSelection::Auto,
            max_steps: None,
        };
        let mut index = 0;

        while index < arguments.len() {
            match arguments[index].as_str() {
                "--train" => {
                    options.train =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--train")?)
                }
                "--validation" => {
                    options.validation = PathBuf::from(argument_value::<String>(
                        arguments,
                        &mut index,
                        "--validation",
                    )?)
                }
                "--output" => {
                    options.output =
                        PathBuf::from(argument_value::<String>(arguments, &mut index, "--output")?)
                }
                "--epochs" => options.epochs = argument_value(arguments, &mut index, "--epochs")?,
                "--batch-size" => {
                    options.batch_size = argument_value(arguments, &mut index, "--batch-size")?
                }
                "--learning-rate" => {
                    options.learning_rate =
                        argument_value(arguments, &mut index, "--learning-rate")?
                }
                "--seed" => options.seed = argument_value(arguments, &mut index, "--seed")?,
                "--device" => {
                    let device = argument_value::<String>(arguments, &mut index, "--device")?;
                    options.device = match device.as_str() {
                        "auto" => DeviceSelection::Auto,
                        "discrete" => DeviceSelection::Discrete,
                        "cpu" => DeviceSelection::Cpu,
                        _ => return Err("device must be auto, discrete or cpu".to_string()),
                    };
                }
                "--max-steps" => {
                    options.max_steps = Some(argument_value(arguments, &mut index, "--max-steps")?)
                }
                unknown => return Err(format!("unknown train option: {unknown}")),
            }
            index += 1;
        }

        if options.epochs == 0 || options.batch_size == 0 {
            return Err("epochs and batch size must be positive".to_string());
        }
        if !options.learning_rate.is_finite() || options.learning_rate <= 0.0 {
            return Err("learning rate must be positive and finite".to_string());
        }
        if options.max_steps == Some(0) {
            return Err("max steps must be positive".to_string());
        }
        Ok(options)
    }
}

pub fn train(options: &TrainOptions) -> Result<TrainStats, String> {
    let train_records = read_records(&options.train).map_err(|error| error.to_string())?;
    let validation_records =
        read_records(&options.validation).map_err(|error| error.to_string())?;
    if train_records.is_empty() || validation_records.is_empty() {
        return Err("train and validation datasets must not be empty".to_string());
    }

    let device = match options.device {
        DeviceSelection::Auto => training_device(false),
        DeviceSelection::Discrete => training_device(true),
        DeviceSelection::Cpu => Device::wgpu(DeviceKind::Cpu)
            .autodiff()
            .gradient_checkpointing(),
    };
    println!("device: {device:?}");
    println!(
        "train records: {}, validation records: {}",
        train_records.len(),
        validation_records.len()
    );

    let mut model = ChessNet::new(&device).train();
    let mut optimizer = AdamConfig::new().init();
    let loss_function = CrossEntropyLossConfig::new().init(&device);
    let mut random = StdRng::seed_from_u64(options.seed);
    let mut indices: Vec<usize> = (0..train_records.len()).collect();
    let mut stats = TrainStats::default();
    let mut best_validation_top1 = f32::NEG_INFINITY;
    let started = Instant::now();

    for epoch in 1..=options.epochs {
        indices.shuffle(&mut random);
        let learning_rate = options.learning_rate
            * f64::from(LR_DECAY_FACTOR.powi((epoch as u32 / LR_DECAY_EPOCHS) as i32));
        let mut epoch_loss = 0.0;
        let mut epoch_samples = 0;
        let mut steps = 0;

        for batch_indices in indices.chunks(options.batch_size) {
            let batch: Vec<&TrainingRecord> = batch_indices
                .iter()
                .map(|index| &train_records[*index])
                .collect();
            let (updated_model, loss, batch_samples) = train_batch(
                model,
                &mut optimizer,
                &loss_function,
                &device,
                &batch,
                learning_rate,
            )?;
            model = updated_model;
            epoch_loss += loss * batch_samples as f32;
            epoch_samples += batch_samples;
            steps += 1;
            stats.steps += 1;

            if stats.steps % LOG_INTERVAL == 0 {
                println!(
                    "epoch {epoch} step {steps} loss {:.4} lr {learning_rate:.6} elapsed {:.1}s",
                    loss,
                    started.elapsed().as_secs_f32()
                );
            }
            if options
                .max_steps
                .is_some_and(|max_steps| stats.steps >= max_steps)
            {
                break;
            }
        }

        let validation = evaluate(&model, &validation_records, &device, options.batch_size)?;
        stats.final_loss = epoch_loss / epoch_samples as f32;
        println!(
            "epoch {epoch}: train_loss {:.4}, val_loss {:.4}, val_top1 {:.2}%, val_top3 {:.2}%, val_wdl {:.2}%",
            stats.final_loss,
            validation.loss,
            validation.top1 * 100.0,
            validation.top3 * 100.0,
            validation.value_accuracy * 100.0
        );

        if validation.top1 > best_validation_top1 {
            best_validation_top1 = validation.top1;
            stats.best_epoch = epoch;
            stats.best_validation_top1 = validation.top1;
            save_model(&model, &options.output)?;
        }

        if options
            .max_steps
            .is_some_and(|max_steps| stats.steps >= max_steps)
        {
            break;
        }
    }

    if stats.best_epoch == 0 {
        save_model(&model, &options.output)?;
    }
    Ok(stats)
}

fn train_batch(
    model: ChessNet,
    optimizer: &mut ModuleOptimizer,
    loss_function: &burn::nn::loss::CrossEntropyLoss,
    device: &Device,
    records: &[&TrainingRecord],
    learning_rate: f64,
) -> Result<(ChessNet, f32, usize), String> {
    let batch_size = records.len();
    let input_values: Vec<f32> = records
        .iter()
        .flat_map(|record| record.input.iter().map(|value| f32::from(*value) / 255.0))
        .collect();
    let legal_values: Vec<bool> = records
        .iter()
        .flat_map(|record| {
            record
                .legal_mask
                .iter()
                .flat_map(|byte| (0..8).map(move |bit| byte & (1 << bit) != 0))
        })
        .collect();
    let policy_targets: Vec<i32> = records
        .iter()
        .map(|record| i32::from(record.target_move))
        .collect();
    let value_targets: Vec<i32> = records
        .iter()
        .map(|record| i32::from(record.result))
        .collect();

    let input = Tensor::<4>::from_data(
        TensorData::new(input_values, [batch_size, INPUT_CHANNELS, 8, 8]),
        device,
    );
    let legal = Tensor::<2, Bool>::from_data(
        TensorData::new(legal_values, [batch_size, POLICY_MOVES]),
        device,
    );
    let policy_targets =
        Tensor::<1, Int>::from_data(TensorData::new(policy_targets, [batch_size]), device);
    let value_targets =
        Tensor::<1, Int>::from_data(TensorData::new(value_targets, [batch_size]), device);
    let (policy_logits, value_logits) = model.forward(input);
    let masked_logits = policy_logits.mask_fill(legal.not(), f32::NEG_INFINITY);
    let policy_loss = loss_function.forward(masked_logits, policy_targets);
    let value_loss = loss_function.forward(value_logits, value_targets);
    let loss = policy_loss + value_loss * VALUE_LOSS_WEIGHT;
    let loss_value = loss
        .to_data()
        .try_to_vec::<f32>()
        .map_err(|error| error.to_string())?
        .first()
        .copied()
        .ok_or_else(|| "empty loss tensor".to_string())?;
    let gradients = loss.backward();
    let gradients = GradientsParams::from_grads(gradients, &model);
    let updated = optimizer.step(learning_rate, model, gradients);
    Ok((updated, loss_value, batch_size))
}

fn evaluate(
    model: &ChessNet,
    records: &[TrainingRecord],
    device: &Device,
    batch_size: usize,
) -> Result<Metrics, String> {
    let model = model.clone().valid();
    let loss_function = CrossEntropyLossConfig::new().init(device);
    let mut metrics = Metrics::default();

    for batch in records.chunks(batch_size) {
        let input_values: Vec<f32> = batch
            .iter()
            .flat_map(|record| record.input.iter().map(|value| f32::from(*value) / 255.0))
            .collect();
        let legal_values: Vec<bool> = batch
            .iter()
            .flat_map(|record| {
                record
                    .legal_mask
                    .iter()
                    .flat_map(|byte| (0..8).map(move |bit| byte & (1 << bit) != 0))
            })
            .collect();
        let policy_targets: Vec<i32> = batch
            .iter()
            .map(|record| i32::from(record.target_move))
            .collect();
        let value_targets: Vec<i32> = batch
            .iter()
            .map(|record| i32::from(record.result))
            .collect();
        let batch_len = batch.len();
        let input = Tensor::<4>::from_data(
            TensorData::new(input_values, [batch_len, INPUT_CHANNELS, 8, 8]),
            device,
        );
        let legal = Tensor::<2, Bool>::from_data(
            TensorData::new(legal_values, [batch_len, POLICY_MOVES]),
            device,
        );
        let policy_target_tensor = Tensor::<1, Int>::from_data(
            TensorData::new(policy_targets.clone(), [batch_len]),
            device,
        );
        let value_target_tensor = Tensor::<1, Int>::from_data(
            TensorData::new(value_targets.clone(), [batch_len]),
            device,
        );
        let (policy_logits, value_logits) = model.forward(input);
        let masked_logits = policy_logits.mask_fill(legal.not(), f32::NEG_INFINITY);
        let policy_loss = loss_function.forward(masked_logits.clone(), policy_target_tensor);
        let value_loss = loss_function.forward(value_logits.clone(), value_target_tensor);
        let loss = policy_loss + value_loss * VALUE_LOSS_WEIGHT;
        let loss_value = loss
            .to_data()
            .try_to_vec::<f32>()
            .map_err(|error| error.to_string())?
            .first()
            .copied()
            .ok_or_else(|| "empty loss tensor".to_string())?;
        let logits = masked_logits
            .to_data()
            .try_to_vec::<f32>()
            .map_err(|error| error.to_string())?;
        let value_logits = value_logits
            .to_data()
            .try_to_vec::<f32>()
            .map_err(|error| error.to_string())?;

        for row in 0..batch_len {
            let start = row * POLICY_MOVES;
            let end = start + POLICY_MOVES;
            let target = policy_targets[row] as usize;
            let target_logit = logits[start + target];
            let better = logits[start..end]
                .iter()
                .filter(|logit| **logit > target_logit)
                .count();
            if better == 0 {
                metrics.top1 += 1.0;
            }
            if better < 3 {
                metrics.top3 += 1.0;
            }

            let value_start = row * VALUE_CLASSES;
            let predicted = value_logits[value_start..value_start + VALUE_CLASSES]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .ok_or_else(|| "empty value head".to_string())?;
            if predicted == value_targets[row] as usize {
                metrics.value_accuracy += 1.0;
            }
        }

        metrics.loss += loss_value * batch_len as f32;
        metrics.samples += batch_len;
    }

    let samples = metrics.samples as f32;
    metrics.loss /= samples;
    metrics.top1 /= samples;
    metrics.top3 /= samples;
    metrics.value_accuracy /= samples;
    Ok(metrics)
}

fn save_model(model: &ChessNet, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    model.save(path)
}

fn argument_value<T: std::str::FromStr>(
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
    use super::{DeviceSelection, TrainOptions};
    use std::path::PathBuf;

    #[test]
    fn parses_training_options() {
        let arguments: Vec<String> = [
            "--train",
            "train.bin",
            "--epochs",
            "3",
            "--batch-size",
            "64",
            "--device",
            "discrete",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let options = TrainOptions::parse(&arguments).unwrap();
        assert_eq!(options.train, PathBuf::from("train.bin"));
        assert_eq!(options.epochs, 3);
        assert_eq!(options.batch_size, 64);
        assert_eq!(options.device, DeviceSelection::Discrete);
    }

    #[test]
    fn rejects_invalid_batch_size() {
        let arguments = ["--batch-size", "0"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(TrainOptions::parse(&arguments).is_err());
    }
}
