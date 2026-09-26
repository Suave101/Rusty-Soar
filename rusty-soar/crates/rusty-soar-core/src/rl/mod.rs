//! Reinforcement Learning (RL) Q-value tuning module for `rusty-soar`.

use alloc::vec::Vec;
use crate::symbol::SymbolId;

/// Represents a Q-value entry mapping a (state, operator) pair to a learned numeric preference.
#[derive(Debug, Clone, PartialEq)]
pub struct QTableEntry {
    /// State symbol handle.
    pub state: SymbolId,
    /// Operator symbol handle.
    pub operator: SymbolId,
    /// Expected discounted cumulative reward (Q-value).
    pub q_value: f32,
}

/// Reinforcement Learning engine managing Q-table updates and temporal difference learning.
#[derive(Debug)]
pub struct ReinforcementLearning {
    /// Learning rate hyperparameter (\alpha).
    pub alpha: f32,
    /// Discount factor hyperparameter (\gamma).
    pub gamma: f32,
    /// Q-table storing state-operator value estimations.
    pub q_table: Vec<QTableEntry>,
    /// Accumulated reward pool for the current decision cycle.
    pub pending_reward: f32,
}

impl Default for ReinforcementLearning {
    fn default() -> Self {
        Self::new(0.1, 0.9)
    }
}

impl ReinforcementLearning {
    /// Creates a new `ReinforcementLearning` engine with specified learning rate and discount factor.
    pub fn new(alpha: f32, gamma: f32) -> Self {
        Self {
            alpha,
            gamma,
            q_table: Vec::new(),
            pending_reward: 0.0,
        }
    }

    /// Fetches the current Q-value for a given state-operator pair, defaulting to 0.0.
    pub fn get_q_value(&self, state: SymbolId, operator: SymbolId) -> f32 {
        self.q_table
            .iter()
            .find(|entry| entry.state == state && entry.operator == operator)
            .map(|entry| entry.q_value)
            .unwrap_or(0.0)
    }

    /// Sets or updates the stored Q-value for a given state-operator pair.
    pub fn set_q_value(&mut self, state: SymbolId, operator: SymbolId, new_value: f32) {
        if let Some(entry) = self
            .q_table
            .iter_mut()
            .find(|entry| entry.state == state && entry.operator == operator)
        {
            entry.q_value = new_value;
        } else {
            self.q_table.push(QTableEntry {
                state,
                operator,
                q_value: new_value,
            });
        }
    }

    /// Deposits a reward signal into the pending reward pool.
    pub fn add_reward(&mut self, reward: f32) {
        self.pending_reward += reward;
    }

    /// Performs a Q-learning Temporal Difference (TD) update on a state-operator transition:
    ///
    /// Q(s, a) = Q(s, a) + \alpha * (r + \gamma * max_a' Q(s', a') - Q(s, a))
    pub fn update_q_value(
        &mut self,
        state: SymbolId,
        operator: SymbolId,
        next_state_max_q: f32,
    ) -> f32 {
        let current_q = self.get_q_value(state, operator);
        let reward = self.pending_reward;

        // Temporal Difference Error
        let td_error = reward + (self.gamma * next_state_max_q) - current_q;
        let new_q = current_q + (self.alpha * td_error);

        self.set_q_value(state, operator, new_q);
        self.pending_reward = 0.0; // Reset reward after update

        new_q
    }

    /// Evaluates candidate operators and returns the maximum Q-value for the candidate set.
    pub fn max_q_value_for_candidates(&self, state: SymbolId, candidates: &[SymbolId]) -> f32 {
        let mut max_q = 0.0f32;
        for &op in candidates {
            let q = self.get_q_value(state, op);
            if q > max_q {
                max_q = q;
            }
        }
        max_q
    }
}