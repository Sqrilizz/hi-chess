use crate::{
    features::{Policy, encode_board, legacy_policy_from_logits, policy_from_logits},
    network::{ChessNet, ChessNetV3, inference_device, predict_model},
};
use burn::tensor::Device;
use chess::{Board, ChessMove};
use std::path::Path;

enum LoadedModel {
    V2(Box<ChessNet>),
    V3(Box<ChessNetV3>),
}

pub struct ModelRunner {
    device: Device,
    model: LoadedModel,
}

impl ModelRunner {
    pub fn new(path: impl AsRef<Path>, discrete_gpu: bool) -> Result<Self, String> {
        let device = inference_device(discrete_gpu);
        let model = match ChessNetV3::load(&path, &device) {
            Ok(model) => LoadedModel::V3(Box::new(model)),
            Err(_) => LoadedModel::V2(Box::new(ChessNet::load(&path, &device)?)),
        };
        Ok(Self { device, model })
    }

    pub fn predict(
        &self,
        board: &Board,
        own_elo: u16,
        opponent_elo: u16,
        halfmove_clock: u16,
        last_move: Option<ChessMove>,
        temperature: f32,
    ) -> Result<Policy, String> {
        let input = encode_board(board, own_elo, opponent_elo, halfmove_clock, last_move);
        match &self.model {
            LoadedModel::V2(model) => {
                let (policy_logits, wdl_logits) =
                    predict_model(model.as_ref(), &self.device, &input)?;
                Ok(legacy_policy_from_logits(
                    board,
                    &policy_logits,
                    wdl_logits,
                    temperature,
                ))
            }
            LoadedModel::V3(model) => {
                let (policy_logits, wdl_logits) =
                    predict_model(model.as_ref(), &self.device, &input)?;
                Ok(policy_from_logits(
                    board,
                    &policy_logits,
                    wdl_logits,
                    temperature,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ModelRunner;

    #[test]
    fn model_runner_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ModelRunner>();
    }
}
