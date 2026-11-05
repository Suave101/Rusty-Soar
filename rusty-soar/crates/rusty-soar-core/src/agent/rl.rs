//! Reinforcement learning (upstream `reinforcement_learning.cpp`): SARSA and Q-learning with
//! eligibility traces over RL rules, the rules whose only action is a numeric-indifferent
//! preference.

use super::SoarAgent;
use crate::preference::{PreferenceType, RlChoice};
use crate::production::compile::{CompiledAction, RhsTerm};
use crate::rl::{DecayMode, LearningPolicy, RlRule, RlSetting};
use crate::soar_parser::SoarPreference;
use crate::symbol::SymbolId;
use crate::wm::SupportType;
use alloc::vec::Vec;

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// Upstream `rl_enabled`: the learning policy when learning is on.
    pub(super) fn rl_learning(&self) -> Option<LearningPolicy> {
        self.rl.params.learning.then_some(self.rl.params.learning_policy)
    }

    /// Applies an `rl --set` setting. Turning learning off clears every goal's RL data, as
    /// upstream's `rl_learning_param::set_value` does through `rl_reset_data`.
    pub fn set_rl(&mut self, setting: RlSetting) {
        let p = &mut self.rl.params;
        match setting {
            RlSetting::Learning(on) => {
                if p.learning && !on {
                    for goal in &mut self.decider.goals {
                        let reward_link = goal.rl.reward_link;
                        goal.rl = crate::rl::RlGoal { reward_link, ..Default::default() };
                    }
                }
                self.rl.params.learning = on;
            }
            RlSetting::DiscountRate(x) => p.discount_rate = x,
            RlSetting::LearningRate(x) => p.learning_rate = x,
            RlSetting::StepSizeParameter(x) => p.step_size_parameter = x,
            RlSetting::MetaLearningRate(x) => p.meta_learning_rate = x,
            RlSetting::LearningPolicy(x) => p.learning_policy = x,
            RlSetting::DecayMode(x) => p.decay_mode = x,
            RlSetting::EtDecayRate(x) => p.et_decay_rate = x,
            RlSetting::EtTolerance(x) => p.et_tolerance = x,
            RlSetting::TemporalExtension(x) => p.temporal_extension = x,
            RlSetting::HrlDiscount(x) => p.hrl_discount = x,
            RlSetting::TemporalDiscount(x) => p.temporal_discount = x,
        }
    }

    /// Upstream `rl_valid_rule`, applied as `make_production` does: a rule whose only action
    /// is a numeric-indifferent preference becomes an RL rule, its expected future reward
    /// starting at the preference's value.
    pub(super) fn register_rl_rule(&mut self, name: &'static str, actions: &[CompiledAction]) {
        let [CompiledAction::Make { preference: SoarPreference::NumericIndifferent, referent, .. }] = actions else {
            return;
        };
        let initial = match referent {
            Some(RhsTerm::Constant(symbol)) => self.number(*symbol).unwrap_or(0.0),
            _ => 0.0,
        };
        self.rl.rules.push(RlRule::new(name, initial));
    }

    pub(super) fn rl_rule_index(&self, name: &str) -> Option<usize> {
        self.rl.rules.iter().position(|r| r.name == name)
    }

    /// The numeric preference value an RL rule's firing makes once learning has changed it.
    pub(super) fn rl_rule_value(&self, name: &str) -> Option<SymbolId> {
        self.rl.rules.iter().find(|r| r.name == name).and_then(|r| r.value)
    }

    fn number(&self, symbol: SymbolId) -> Option<f64> {
        self.symbols.number(symbol)
    }

    /// Values of `(id ^attr *)` slot WMEs: not acceptable-preference WMEs or input WMEs, which
    /// upstream keeps outside slots.
    fn slot_values(&self, id: SymbolId, attr: SymbolId) -> Vec<SymbolId> {
        self.wm
            .iter()
            .filter(|w| w.id == id && w.attr == attr && !w.acceptable && w.support != SupportType::Input)
            .map(|w| w.val)
            .collect()
    }

    /// Upstream `rl_tabulate_reward_value_for_goal`: adds the goal's `^reward-link.reward.value`
    /// numbers, discounted by the time since the last update, to its pending reward.
    pub(super) fn rl_tabulate_reward_value_for_goal(&mut self, goal: usize) {
        if self.decider.goals[goal].rl.prev_op_rl_rules.is_empty() {
            return;
        }
        let params = self.rl.params;
        let mut reward = 0.0;
        if let Some(link) = self.decider.goals[goal].rl.reward_link {
            let reward_attr = self.symbols.predefined.reward;
            let value_attr = self.symbols.predefined.value;
            for r in self.slot_values(link, reward_attr) {
                if self.symbols.is_identifier(r) {
                    for v in self.slot_values(r, value_attr) {
                        if let Some(n) = self.number(v) {
                            reward += n;
                        }
                    }
                }
            }
        }
        let bottom = goal + 1 == self.decider.goals.len();
        let data = &mut self.decider.goals[goal].rl;
        let mut effective_age = data.hrl_age;
        if params.temporal_discount {
            effective_age += data.gap_age;
        }
        data.reward += reward * libm::pow(params.discount_rate, f64::from(effective_age));
        if !bottom && params.hrl_discount {
            data.hrl_age += 1;
        }
    }

    /// Upstream `rl_tabulate_reward_values`: every goal, top first.
    pub(super) fn rl_tabulate_reward_values(&mut self) {
        for goal in 0..self.decider.goals.len() {
            self.rl_tabulate_reward_value_for_goal(goal);
        }
    }

    /// Applies the reinforcement-learning part of a decision, as upstream does inside
    /// `run_preference_semantics`.
    pub(super) fn rl_apply_choice(&mut self, goal: usize, choice: RlChoice) {
        self.rl_tabulate_reward_values();
        self.rl_perform_update(goal, choice.update_value, choice.update_rl, true);
        if choice.watkins_clear {
            self.decider.goals[goal].rl.traces.clear();
        }
    }

    /// Upstream `rl_store_data`, when `operator` is selected for `goal` with value
    /// `selected_value`: the RL rules supporting it become the ones the next update credits.
    pub(super) fn rl_store_data(&mut self, goal: usize, operator: SymbolId, selected_value: f64, rho: f64) {
        let id = self.decider.goals[goal].id;
        let operator_attr = self.symbols.predefined.operator;
        let supporting: Vec<usize> = self
            .slot_preference_records(id, operator_attr)
            .into_iter()
            .filter(|p| p.value == operator && matches!(p.preference_type, PreferenceType::NumericIndifferent(_)))
            .filter_map(|p| p.production.and_then(|name| self.rl_rule_index(name)))
            .collect();
        let using_gaps = self.rl.params.temporal_extension;
        let data = &mut self.decider.goals[goal].rl;
        if !supporting.is_empty() {
            if !data.prev_op_rl_rules.is_empty() {
                data.prev_op_rl_rules.clear();
            }
            data.prev_op_rl_rules.extend(supporting);
            data.previous_q = selected_value;
            data.rho = rho;
        } else if !using_gaps {
            data.prev_op_rl_rules.clear();
            data.previous_q = selected_value;
            data.rho = 1.0;
        } else if !data.prev_op_rl_rules.is_empty() {
            data.gap_age += 1;
        }
    }

    /// Upstream `rl_perform_update`: credits the RL rules that supported the goal's previous
    /// operator with the reward since then plus the discounted value `op_value` of what follows.
    ///
    /// As upstream, every traced rule's current preferences take the value computed for the
    /// last rule updated.
    pub(super) fn rl_perform_update(&mut self, goal: usize, op_value: f64, op_rl: bool, update_efr: bool) {
        let p = self.rl.params;
        if p.temporal_extension && !op_rl {
            return;
        }
        let has_lower_goal = goal + 1 < self.decider.goals.len();
        let data = &mut self.decider.goals[goal].rl;
        if !data.prev_op_rl_rules.is_empty() {
            let mut effective_age = data.hrl_age + 1;
            if p.temporal_discount {
                effective_age += data.gap_age;
            }
            let discount = libm::pow(p.discount_rate, f64::from(effective_age));

            // Decay traces, dropping those below the tolerance.
            if p.et_decay_rate == 0.0 {
                data.traces.clear();
            } else {
                for (_, trace) in data.traces.iter_mut() {
                    *trace *= p.et_decay_rate;
                    *trace *= discount;
                }
                data.traces.retain(|(_, trace)| *trace >= p.et_tolerance);
            }
            if p.learning_policy.gq() {
                for (_, trace) in data.traces.iter_mut() {
                    *trace *= data.rho;
                }
            }

            // Trace the rules that supported the previous operator. Upstream's `I` is zero for
            // a goal with a subgoal, under hierarchical RL.
            let i = if has_lower_goal { 0.0 } else { 1.0 };
            let increment = i * (1.0 / data.prev_op_rl_rules.len() as f64);
            let mut sum_old_ecr = 0.0;
            let mut sum_old_efr = 0.0;
            let mut dot_w_phi = 0.0;
            for &rule in &data.prev_op_rl_rules {
                sum_old_ecr += self.rl.rules[rule].ecr;
                sum_old_efr += self.rl.rules[rule].efr;
                dot_w_phi += self.rl.rules[rule].gql;
                match data.traces.binary_search_by_key(&rule, |(r, _)| *r) {
                    Ok(at) => data.traces[at].1 += increment,
                    Err(at) => data.traces.insert(at, (rule, increment)),
                }
            }

            let delta_t = (data.reward + discount * op_value) - (sum_old_ecr + sum_old_efr);
            let dot_w_e: f64 = data.traces.iter().map(|(rule, trace)| self.rl.rules[*rule].gql * trace).sum();
            let reward = data.reward;
            let mut new_combined = 0.0;
            for &(rule, trace) in &data.traces {
                let r = &mut self.rl.rules[rule];
                let adjusted_alpha = match p.decay_mode {
                    DecayMode::Exponential => 1.0 / (r.update_count + 1.0),
                    DecayMode::Logarithmic => 1.0 / (libm::log(r.update_count + 1.0) + 1.0),
                    DecayMode::DeltaBarDelta => {
                        r.delta_bar_delta_beta += p.meta_learning_rate * delta_t * r.delta_bar_delta_h;
                        let alpha = libm::exp(r.delta_bar_delta_beta);
                        let decay_term = (1.0 - alpha).max(0.0);
                        r.delta_bar_delta_h = r.delta_bar_delta_h * decay_term + alpha * delta_t;
                        alpha
                    }
                    DecayMode::Normal => p.learning_rate,
                };
                let delta_ecr = adjusted_alpha * trace * (reward - sum_old_ecr);
                let delta_efr = if update_efr { adjusted_alpha * trace * ((discount * op_value) - sum_old_efr) } else { 0.0 };
                let new_ecr = r.ecr + delta_ecr;
                let new_efr = r.efr + delta_efr;
                new_combined = new_ecr + new_efr;
                r.gql += p.step_size_parameter * (delta_ecr + delta_efr);
                r.ecr = new_ecr;
                r.efr = new_efr;
                r.update_count += 1.0;
                r.value = Some(self.symbols.intern_float(new_combined));
            }

            if p.learning_policy.gq() {
                // Upstream: each numeric preference now in the goal's operator slot corrects its
                // rule's expected future reward, and the previous operator's rules their weights.
                let goal_id = self.decider.goals[goal].id;
                let operator = self.symbols.predefined.operator;
                let correction = p.learning_rate * p.discount_rate * (1.0 - p.et_decay_rate) * dot_w_e;
                for pref in self.preferences.iter().filter(|q| q.id == goal_id && q.attr == operator) {
                    if let (PreferenceType::NumericIndifferent(_), Some(name)) = (pref.preference_type, pref.production) {
                        if let Some(rule) = self.rl.rules.iter_mut().find(|r| r.name == name) {
                            rule.efr -= correction;
                        }
                    }
                }
                let data = &self.decider.goals[goal].rl;
                for &rule in &data.prev_op_rl_rules {
                    self.rl.rules[rule].gql -= p.learning_rate * p.step_size_parameter * dot_w_phi;
                }
            }
            let data = &self.decider.goals[goal].rl;
            let traced: Vec<&'static str> = data.traces.iter().map(|(rule, _)| self.rl.rules[*rule].name).collect();
            if !traced.is_empty() {
                let value = self.symbols.intern_float(new_combined);
                for pref in self.preferences.iter_mut() {
                    if pref.production.is_some_and(|name| traced.contains(&name)) {
                        if let PreferenceType::NumericIndifferent(_) = pref.preference_type {
                            pref.preference_type = PreferenceType::NumericIndifferent(value);
                        }
                    }
                }
            }
        }
        let data = &mut self.decider.goals[goal].rl;
        data.gap_age = 0;
        data.hrl_age = 0;
        data.reward = 0.0;
    }

    /// The update upstream makes when a substate goes away: it sees only reward, as there is
    /// no next state.
    pub(super) fn rl_update_for_removed_goal(&mut self, goal: usize) {
        self.rl_tabulate_reward_value_for_goal(goal);
        self.rl_perform_update(goal, 0.0, true, false);
        self.decider.goals[goal].rl.prev_op_rl_rules.clear();
    }
}
