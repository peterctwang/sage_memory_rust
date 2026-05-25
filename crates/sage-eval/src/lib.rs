//! Retrieval evaluation metrics + sample driver.
//!
//! All metric functions are pure and depend only on `sage-core::DocId`.

pub mod metrics;
pub mod reward_loop;
pub mod runner;

pub use metrics::{f1_at_k, mrr, precision_at_k, recall_at_k};
pub use reward_loop::{compute_reward_for_sample, JudgeInputs};
pub use runner::{EvalReport, EvalRunner, EvalSample};
