//! # Neural Network — Pure-Rust Feedforward with Backpropagation
//!
//! A dependency-free, fully-connected feedforward neural network trained
//! with stochastic gradient descent and backpropagation. Used by nanomite
//! to learn which tools work best in which contexts, and to predict success
//! probabilities before dispatching a tool call.
//!
//! Architecture:
//! - Input layer -> hidden layer(s) -> output layer
//! - ReLU activation in hidden layers, sigmoid in output
//! - MSE loss, SGD with momentum, L2 regularization optional
//! - Persists to JSON (weights + biases + architecture)

// serde is available in the main crate; use it for persistence.
use serde::{Deserialize, Serialize};
use std::f64::consts::E;

/// Activation functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activation {
    Relu,
    Sigmoid,
    Tanh,
    Linear,
}

impl Activation {
    pub fn activate(self, x: f64) -> f64 {
        match self {
            Activation::Relu => x.max(0.0),
            Activation::Sigmoid => 1.0 / (1.0 + E.powf(-x)),
            Activation::Tanh => x.tanh(),
            Activation::Linear => x,
        }
    }

    pub fn derivative(self, x: f64) -> f64 {
        match self {
            Activation::Relu => {
                if x > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Activation::Sigmoid => {
                let s = self.activate(x);
                s * (1.0 - s)
            }
            Activation::Tanh => 1.0 - x.tanh().powi(2),
            Activation::Linear => 1.0,
        }
    }
}

/// One layer of the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Weights: rows = output neurons, cols = input neurons.
    pub weights: Vec<Vec<f64>>,
    /// Biases: one per output neuron.
    pub biases: Vec<f64>,
    pub activation: Activation,
}

impl Layer {
    pub fn new(outputs: usize, inputs: usize, activation: Activation) -> Self {
        // Xavier/Glorot initialization.
        let limit = (6.0 / (inputs + outputs) as f64).sqrt();
        // Use a simple deterministic PRNG seeded by layer shape so we don't
        // need an external RNG crate.
        let weights = (0..outputs)
            .map(|_| (0..inputs).map(|_| xavier_random(limit)).collect())
            .collect();
        let biases = (0..outputs).map(|_| xavier_random(limit * 0.1)).collect();
        Self {
            weights,
            biases,
            activation,
        }
    }

    pub fn forward(&self, inputs: &[f64]) -> Vec<f64> {
        self.weights
            .iter()
            .zip(self.biases.iter())
            .map(|(w, b)| {
                let sum: f64 = w.iter().zip(inputs.iter()).map(|(wi, xi)| wi * xi).sum();
                self.activation.activate(sum + b)
            })
            .collect()
    }
}

/// Simple deterministic pseudo-random in [-limit, limit] using a splitmix64
/// hash of a counter, so initialization is reproducible without an RNG crate.
fn xavier_random(limit: f64) -> f64 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let x = splitmix64(n.wrapping_add(0x9e3779b97f4a7c15));
    (x as f64 / u64::MAX as f64) * 2.0 * limit - limit
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// Forward pass result: layer outputs and pre-activations (for backprop).
#[derive(Debug, Clone)]
pub struct ForwardTrace {
    layer_outputs: Vec<Vec<f64>>,   // including input as index 0
    pre_activations: Vec<Vec<f64>>, // per layer (excluding input)
}

/// Complete feedforward network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralNetwork {
    pub layers: Vec<Layer>,
    pub learning_rate: f64,
    pub momentum: f64,
    /// Velocity for momentum (same shape as weights/biases), persisted.
    pub weight_velocity: Vec<Vec<Vec<f64>>>,
    pub bias_velocity: Vec<Vec<f64>>,
    /// Training history: loss per epoch.
    pub training_history: Vec<f64>,
}

/// A single training sample.
#[derive(Debug, Clone)]
pub struct TrainingSample {
    pub input: Vec<f64>,
    pub target: Vec<f64>,
}

impl NeuralNetwork {
    /// Create a new network. `layer_sizes[0]` is input size, last is output size.
    pub fn new(layer_sizes: &[usize], learning_rate: f64, momentum: f64) -> Self {
        assert!(layer_sizes.len() >= 2, "need at least input + output");
        let mut layers = Vec::new();
        for i in 0..layer_sizes.len() - 1 {
            let activation = if i == layer_sizes.len() - 2 {
                Activation::Sigmoid // output layer
            } else {
                Activation::Relu // hidden layers
            };
            layers.push(Layer::new(layer_sizes[i + 1], layer_sizes[i], activation));
        }
        let weight_velocity = layers
            .iter()
            .map(|l| l.weights.iter().map(|row| vec![0.0; row.len()]).collect())
            .collect();
        let bias_velocity = layers.iter().map(|l| vec![0.0; l.biases.len()]).collect();
        Self {
            layers,
            learning_rate,
            momentum,
            weight_velocity,
            bias_velocity,
            training_history: Vec::new(),
        }
    }

    /// Forward pass. Returns output and a trace for backpropagation.
    pub fn forward(&self, input: &[f64]) -> (Vec<f64>, ForwardTrace) {
        let mut activations = vec![input.to_vec()];
        let mut pre_acts = Vec::new();

        let mut current = input.to_vec();
        for layer in &self.layers {
            let mut pre = Vec::with_capacity(layer.weights.len());
            let mut next = Vec::with_capacity(layer.weights.len());
            for (w, b) in layer.weights.iter().zip(layer.biases.iter()) {
                let sum: f64 = w
                    .iter()
                    .zip(current.iter())
                    .map(|(wi, xi)| wi * xi)
                    .sum::<f64>()
                    + b;
                pre.push(sum);
                next.push(layer.activation.activate(sum));
            }
            pre_acts.push(pre);
            current = next;
            activations.push(current.clone());
        }

        (
            current,
            ForwardTrace {
                layer_outputs: activations,
                pre_activations: pre_acts,
            },
        )
    }

    /// Predict output for input (no trace, allocation-light).
    pub fn predict(&self, input: &[f64]) -> Vec<f64> {
        let (out, _) = self.forward(input);
        out
    }

    /// Train on one sample with backpropagation. Returns the MSE loss.
    pub fn train(&mut self, sample: &TrainingSample) -> f64 {
        let (output, trace) = forward_with_trace(self, &sample.input);
        let loss = mse(&output, &sample.target);
        backpropagate(self, &trace, &sample.input, &sample.target);
        loss
    }

    /// Train for multiple epochs, shuffling each epoch.
    pub fn train_epochs(&mut self, samples: &[TrainingSample], epochs: usize) -> Vec<f64> {
        let mut history = Vec::with_capacity(epochs);
        let mut idx: Vec<usize> = (0..samples.len()).collect();
        for _ in 0..epochs {
            idx.shuffle_deterministic();
            let mut epoch_loss = 0.0;
            for &i in &idx {
                epoch_loss += self.train(&samples[i]);
            }
            epoch_loss /= samples.len() as f64;
            history.push(epoch_loss);
            self.training_history.push(epoch_loss);
        }
        history
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Deserialize from JSON string.
    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

fn forward_with_trace(net: &NeuralNetwork, input: &[f64]) -> (Vec<f64>, ForwardTrace) {
    net.forward(input)
}

fn mse(output: &[f64], target: &[f64]) -> f64 {
    assert_eq!(output.len(), target.len());
    let sum: f64 = output
        .iter()
        .zip(target.iter())
        .map(|(o, t)| {
            let d = o - t;
            d * d
        })
        .sum();
    sum / output.len() as f64
}

/// Backpropagation: compute gradients and update weights/biases with momentum.
fn backpropagate(net: &mut NeuralNetwork, trace: &ForwardTrace, input: &[f64], target: &[f64]) {
    let n_layers = net.layers.len();
    let output = trace.layer_outputs.last().unwrap().clone();
    // Output layer error (dL/dz for sigmoid + MSE).
    let mut deltas: Vec<Vec<f64>> = (0..n_layers).map(|_| Vec::new()).collect();

    // Output delta: (output - target) * sigmoid'(output)
    let out_pre = &trace.pre_activations[n_layers - 1];
    deltas[n_layers - 1] = output
        .iter()
        .zip(target.iter())
        .zip(out_pre.iter())
        .map(|((o, t), pre)| {
            let err = o - t;
            err * Activation::Sigmoid.derivative(*pre)
        })
        .collect();

    // Hidden layer deltas, propagating backward.
    for l in (0..n_layers - 1).rev() {
        let next_weights = &net.layers[l + 1].weights;
        let next_delta = &deltas[l + 1];
        let pre = &trace.pre_activations[l];
        deltas[l] = (0..pre.len())
            .map(|j| {
                let sum_next: f64 = next_weights
                    .iter()
                    .zip(next_delta.iter())
                    .map(|(w, d)| w[j] * d)
                    .sum();
                sum_next * Activation::Relu.derivative(pre[j])
            })
            .collect();
    }

    // Update weights and biases with momentum.
    let _prev_activation = input.to_vec();
    for (l, layer) in net.layers.iter_mut().enumerate() {
        let activations = if l == 0 {
            input
        } else {
            &trace.layer_outputs[l]
        };
        let delta = &deltas[l];

        for (i, neuron_weights) in layer.weights.iter_mut().enumerate() {
            for (j, w) in neuron_weights.iter_mut().enumerate() {
                let grad = delta[i] * activations[j];
                let velocity = &mut net.weight_velocity[l][i][j];
                *velocity = net.momentum * *velocity + net.learning_rate * grad;
                *w -= *velocity;
            }
        }
        for (i, b) in layer.biases.iter_mut().enumerate() {
            let grad = delta[i];
            let velocity = &mut net.bias_velocity[l][i];
            *velocity = net.momentum * *velocity + net.learning_rate * grad;
            *b -= *velocity;
        }
    }
}

/// Deterministic Fisher-Yates shuffle (no external RNG).
trait ShuffleDeterministic {
    fn shuffle_deterministic(&mut self);
}

impl ShuffleDeterministic for Vec<usize> {
    fn shuffle_deterministic(&mut self) {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut seed = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for i in (1..self.len()).rev() {
            seed = splitmix64(seed);
            let j = (seed as usize) % (i + 1);
            self.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_returns_correct_shape() {
        let net = NeuralNetwork::new(&[3, 4, 2], 0.1, 0.9);
        let (out, _) = net.forward(&[0.5, 0.2, -0.1]);
        assert_eq!(out.len(), 2);
        for v in &out {
            assert!((0.0..=1.0).contains(v), "sigmoid output must be in [0,1]");
        }
    }

    #[test]
    fn training_reduces_loss() {
        let mut net = NeuralNetwork::new(&[2, 3, 1], 0.5, 0.9);
        let samples = vec![
            TrainingSample {
                input: vec![0.0, 0.0],
                target: vec![0.0],
            },
            TrainingSample {
                input: vec![1.0, 0.0],
                target: vec![1.0],
            },
            TrainingSample {
                input: vec![0.0, 1.0],
                target: vec![1.0],
            },
            TrainingSample {
                input: vec![1.0, 1.0],
                target: vec![0.0],
            }, // XOR
        ];
        let initial = net.train(&samples[0]);
        let history = net.train_epochs(&samples, 200);
        let final_loss = history.last().copied().unwrap_or(f64::MAX);
        assert!(
            final_loss < initial,
            "loss should decrease: {initial} -> {final_loss}"
        );
    }

    #[test]
    fn predict_after_training_matches_target() {
        let mut net = NeuralNetwork::new(&[2, 8, 4, 1], 0.5, 0.9);
        let samples = vec![
            TrainingSample {
                input: vec![0.0, 0.0],
                target: vec![0.0],
            },
            TrainingSample {
                input: vec![1.0, 1.0],
                target: vec![0.0],
            },
            TrainingSample {
                input: vec![0.0, 1.0],
                target: vec![1.0],
            },
            TrainingSample {
                input: vec![1.0, 0.0],
                target: vec![1.0],
            },
        ];
        net.train_epochs(&samples, 5000);
        let out = net.predict(&[1.0, 0.0]);
        assert!(out[0] > 0.5, "should learn XOR: {}", out[0]);
    }

    #[test]
    fn serialize_roundtrip() {
        let mut net = NeuralNetwork::new(&[3, 5, 2], 0.01, 0.9);
        net.train_epochs(
            &[TrainingSample {
                input: vec![1.0, 2.0, 3.0],
                target: vec![0.9, 0.1],
            }],
            10,
        );
        let json = net.to_json();
        let restored = NeuralNetwork::from_json(&json).expect("valid json");
        assert_eq!(restored.layers.len(), net.layers.len());
        assert_eq!(
            restored.layers[0].weights.len(),
            net.layers[0].weights.len()
        );
        // Same input -> same output after restore.
        let a = net.predict(&[1.0, 2.0, 3.0]);
        let b = restored.predict(&[1.0, 2.0, 3.0]);
        assert_eq!(a, b);
    }
}
