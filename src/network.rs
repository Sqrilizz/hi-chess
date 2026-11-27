use crate::features::{INPUT_BYTES, INPUT_CHANNELS, POLICY_MOVES, VALUE_CLASSES};
use burn::{
    nn::{
        GroupNorm, GroupNormConfig, Linear, LinearConfig, PaddingConfig2d, Relu,
        conv::{Conv2d, Conv2dConfig},
    },
    prelude::*,
    tensor::DeviceKind,
};

pub const CHANNELS: usize = 12;
pub const BLOCKS: usize = 4;
pub const GROUPS: usize = 4;
pub const WDL_HIDDEN: usize = 128;
pub const V3_CHANNELS: usize = 64;
pub const V3_BLOCKS: usize = 8;
pub const V3_GROUPS: usize = 8;
pub const V3_POLICY_CHANNELS: usize = 4;
pub const V3_WDL_HIDDEN: usize = 256;

pub trait ChessModel: Module + Clone + Send + Sync + 'static {
    fn forward_logits(&self, input: Tensor<4>) -> (Tensor<2>, Tensor<2>);
    fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), String>;
}

impl ChessModel for ChessNet {
    fn forward_logits(&self, input: Tensor<4>) -> (Tensor<2>, Tensor<2>) {
        self.forward(input)
    }

    fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        self.clone()
            .save_file(path)
            .map_err(|error| error.to_string())
    }
}

#[derive(Module, Debug)]
pub struct ChessNet {
    stem: Conv2d,
    stem_norm: GroupNorm,
    blocks: Vec<ResidualBlock>,
    policy: Linear,
    wdl_hidden: Linear,
    wdl: Linear,
    relu: Relu,
}

#[derive(Module, Debug)]
struct ResidualBlock {
    conv1: Conv2d,
    norm1: GroupNorm,
    conv2: Conv2d,
    norm2: GroupNorm,
    relu: Relu,
}

impl ChessNet {
    pub fn new(device: &Device) -> Self {
        Self {
            stem: convolution(INPUT_CHANNELS, CHANNELS, device),
            stem_norm: GroupNormConfig::new(GROUPS, CHANNELS).init(device),
            blocks: (0..BLOCKS).map(|_| ResidualBlock::new(device)).collect(),
            policy: LinearConfig::new(CHANNELS * 64, POLICY_MOVES).init(device),
            wdl_hidden: LinearConfig::new(CHANNELS * 64, WDL_HIDDEN).init(device),
            wdl: LinearConfig::new(WDL_HIDDEN, VALUE_CLASSES).init(device),
            relu: Relu::new(),
        }
    }

    pub fn forward(&self, input: Tensor<4>) -> (Tensor<2>, Tensor<2>) {
        let mut features = self
            .relu
            .forward(self.stem_norm.forward(self.stem.forward(input)));

        for block in &self.blocks {
            features = block.forward(features);
        }

        let flattened: Tensor<2> = features.flatten(1, 3);
        let policy = self.policy.forward(flattened.clone());
        let hidden = self.relu.forward(self.wdl_hidden.forward(flattened));
        let wdl = self.wdl.forward(hidden);
        (policy, wdl)
    }

    pub fn predict(
        &self,
        device: &Device,
        input: &[u8; INPUT_BYTES],
    ) -> Result<([f32; POLICY_MOVES], [f32; VALUE_CLASSES]), String> {
        predict_model(self, device, input)
    }

    pub fn load(path: impl AsRef<std::path::Path>, device: &Device) -> Result<Self, String> {
        ChessNet::new(device)
            .valid()
            .try_load_file(path)
            .map_err(|error| error.to_string())
    }
}

impl ResidualBlock {
    fn new(device: &Device) -> Self {
        Self {
            conv1: convolution(CHANNELS, CHANNELS, device),
            norm1: GroupNormConfig::new(GROUPS, CHANNELS).init(device),
            conv2: convolution(CHANNELS, CHANNELS, device),
            norm2: GroupNormConfig::new(GROUPS, CHANNELS).init(device),
            relu: Relu::new(),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let residual = self
            .relu
            .forward(self.norm1.forward(self.conv1.forward(input.clone())));
        let residual = self.norm2.forward(self.conv2.forward(residual));
        self.relu.forward(input + residual)
    }
}

#[derive(Module, Debug)]
pub struct ChessNetV3 {
    stem: Conv2d,
    stem_norm: GroupNorm,
    blocks: Vec<ResidualBlockV3>,
    policy_conv: Conv2d,
    policy: Linear,
    wdl_hidden: Linear,
    wdl: Linear,
    relu: Relu,
}

#[derive(Module, Debug)]
struct ResidualBlockV3 {
    conv1: Conv2d,
    norm1: GroupNorm,
    conv2: Conv2d,
    norm2: GroupNorm,
    relu: Relu,
}

impl ChessModel for ChessNetV3 {
    fn forward_logits(&self, input: Tensor<4>) -> (Tensor<2>, Tensor<2>) {
        self.forward(input)
    }

    fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        self.clone()
            .save_file(path)
            .map_err(|error| error.to_string())
    }
}

impl ChessNetV3 {
    pub fn new(device: &Device) -> Self {
        Self {
            stem: convolution(INPUT_CHANNELS, V3_CHANNELS, device),
            stem_norm: GroupNormConfig::new(V3_GROUPS, V3_CHANNELS).init(device),
            blocks: (0..V3_BLOCKS)
                .map(|_| ResidualBlockV3::new(device))
                .collect(),
            policy_conv: Conv2dConfig::new([V3_CHANNELS, V3_POLICY_CHANNELS], [1, 1])
                .with_bias(false)
                .init(device),
            policy: LinearConfig::new(V3_POLICY_CHANNELS * 64, POLICY_MOVES).init(device),
            wdl_hidden: LinearConfig::new(V3_CHANNELS * 64, V3_WDL_HIDDEN).init(device),
            wdl: LinearConfig::new(V3_WDL_HIDDEN, VALUE_CLASSES).init(device),
            relu: Relu::new(),
        }
    }

    pub fn forward(&self, input: Tensor<4>) -> (Tensor<2>, Tensor<2>) {
        let mut features = self
            .relu
            .forward(self.stem_norm.forward(self.stem.forward(input)));

        for block in &self.blocks {
            features = block.forward(features);
        }

        let policy_features = self
            .relu
            .forward(self.policy_conv.forward(features.clone()))
            .flatten(1, 3);
        let policy = self.policy.forward(policy_features);
        let value_features: Tensor<2> = features.flatten(1, 3);
        let hidden = self.relu.forward(self.wdl_hidden.forward(value_features));
        let wdl = self.wdl.forward(hidden);
        (policy, wdl)
    }

    pub fn load(path: impl AsRef<std::path::Path>, device: &Device) -> Result<Self, String> {
        ChessNetV3::new(device)
            .valid()
            .try_load_file(path)
            .map_err(|error| error.to_string())
    }

    pub fn load_for_training(
        path: impl AsRef<std::path::Path>,
        device: &Device,
    ) -> Result<Self, String> {
        ChessNetV3::new(device)
            .train()
            .try_load_file(path)
            .map_err(|error| error.to_string())
    }
}

impl ResidualBlockV3 {
    fn new(device: &Device) -> Self {
        Self {
            conv1: convolution(V3_CHANNELS, V3_CHANNELS, device),
            norm1: GroupNormConfig::new(V3_GROUPS, V3_CHANNELS).init(device),
            conv2: convolution(V3_CHANNELS, V3_CHANNELS, device),
            norm2: GroupNormConfig::new(V3_GROUPS, V3_CHANNELS).init(device),
            relu: Relu::new(),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let residual = self
            .relu
            .forward(self.norm1.forward(self.conv1.forward(input.clone())));
        let residual = self.norm2.forward(self.conv2.forward(residual));
        self.relu.forward(input + residual)
    }
}

pub fn predict_model<M: ChessModel>(
    model: &M,
    device: &Device,
    input: &[u8; INPUT_BYTES],
) -> Result<([f32; POLICY_MOVES], [f32; VALUE_CLASSES]), String> {
    let values: Vec<f32> = input
        .iter()
        .map(|value| f32::from(*value) / 255.0)
        .collect();
    let tensor = Tensor::<4>::from_data(TensorData::new(values, [1, INPUT_CHANNELS, 8, 8]), device);
    let (policy, wdl) = model.forward_logits(tensor);
    let policy_values = policy
        .to_data()
        .try_to_vec::<f32>()
        .map_err(|error| error.to_string())?;
    let wdl_values = wdl
        .to_data()
        .try_to_vec::<f32>()
        .map_err(|error| error.to_string())?;

    if policy_values.len() != POLICY_MOVES || wdl_values.len() != VALUE_CLASSES {
        return Err("unexpected neural network output shape".to_string());
    }

    let mut policy_logits = [0.0; POLICY_MOVES];
    policy_logits.copy_from_slice(&policy_values);
    let mut wdl_logits = [0.0; VALUE_CLASSES];
    wdl_logits.copy_from_slice(&wdl_values);
    Ok((policy_logits, wdl_logits))
}

fn convolution(input_channels: usize, output_channels: usize, device: &Device) -> Conv2d {
    Conv2dConfig::new([input_channels, output_channels], [3, 3])
        .with_padding(PaddingConfig2d::Same)
        .with_bias(false)
        .init(device)
}

pub fn inference_device(discrete_gpu: bool) -> Device {
    let kind = if discrete_gpu {
        DeviceKind::DiscreteGpu(0)
    } else {
        DeviceKind::DefaultDevice
    };
    Device::wgpu(kind)
}

pub fn training_device(discrete_gpu: bool) -> Device {
    inference_device(discrete_gpu)
        .autodiff()
        .gradient_checkpointing()
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCKS, CHANNELS, GROUPS, V3_BLOCKS, V3_CHANNELS, V3_GROUPS, V3_POLICY_CHANNELS,
        V3_WDL_HIDDEN, WDL_HIDDEN,
    };

    #[test]
    fn architecture_dimensions_are_valid() {
        const {
            assert!(CHANNELS.is_multiple_of(GROUPS));
            assert!(BLOCKS > 0);
            assert!(WDL_HIDDEN > 0);
            assert!(V3_CHANNELS.is_multiple_of(V3_GROUPS));
            assert!(V3_BLOCKS > 0);
            assert!(V3_WDL_HIDDEN > 0);
            assert!(V3_POLICY_CHANNELS > 0);
        }
    }

    #[test]
    fn v3_policy_head_uses_a_bottleneck() {
        const {
            assert!(V3_POLICY_CHANNELS < V3_CHANNELS);
            assert!(V3_CHANNELS >= 64);
        }
    }
}
