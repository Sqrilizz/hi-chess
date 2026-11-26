use crate::{
    features::{Policy, encode_board, policy_from_logits},
    network::{ChessNet, inference_device},
};
use burn::tensor::Device;
use chess::{Board, ChessMove};
use std::path::Path;

pub struct ModelRunner {
    device: Device,
    model: ChessNet,
}

impl ModelRunner {
    pub fn new(path: impl AsRef<Path>, discrete_gpu: bool) -> Result<Self, String> {
        let device = inference_device(discrete_gpu);
        let model = ChessNet::load(path, &device)?;
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
        let (policy_logits, wdl_logits) = self.model.predict(&self.device, &input)?;
        Ok(policy_from_logits(
            board,
            &policy_logits,
            wdl_logits,
            temperature,
        ))
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
