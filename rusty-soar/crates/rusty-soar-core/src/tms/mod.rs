//! Truth Maintenance System (TMS) dependency tracking for automated I-support retractions.

use crate::wm::WmeKey;
use alloc::vec;
use alloc::vec::Vec;

/// Represents a rule instantiation justification that supports one or more I-supported WMEs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Justification {
    /// Identifier label of the production rule that fired.
    pub rule_name: &'static str,
    /// Set of WME handles that formed the LHS conditions for this rule firing.
    pub supporting_wmes: Vec<WmeKey>,
    /// Set of I-supported WME handles asserted into Working Memory by this rule firing.
    pub derived_wmes: Vec<WmeKey>,
}

/// Truth Maintenance System tracking dependency graphs between WMEs and rule instantiations.
#[derive(Debug, Default)]
pub struct TruthMaintenanceSystem {
    justifications: Vec<Justification>,
}

impl TruthMaintenanceSystem {
    /// Creates a new `TruthMaintenanceSystem` instance.
    pub fn new() -> Self {
        Self {
            justifications: Vec::new(),
        }
    }

    /// Records a new justification mapping supporting antecedent WMEs to derived consequent WMEs.
    pub fn add_justification(
        &mut self,
        rule_name: &'static str,
        supporting_wmes: Vec<WmeKey>,
        derived_wmes: Vec<WmeKey>,
    ) {
        self.justifications.push(Justification {
            rule_name,
            supporting_wmes,
            derived_wmes,
        });
    }

    /// Evaluates retracting a WME handle and returns all downstream I-supported WMEs that lost support.
    pub fn process_retraction(&mut self, retracted_wme: WmeKey) -> Vec<WmeKey> {
        let mut wmes_to_retract = Vec::new();
        let mut queue = vec![retracted_wme];

        while let Some(wme_key) = queue.pop() {
            let mut i = 0;
            while i < self.justifications.len() {
                if self.justifications[i].supporting_wmes.contains(&wme_key) {
                    let just = self.justifications.remove(i);
                    for &derived in &just.derived_wmes {
                        let still_supported = self
                            .justifications
                            .iter()
                            .any(|remaining| remaining.derived_wmes.contains(&derived));
                        if !still_supported && !wmes_to_retract.contains(&derived) {
                            wmes_to_retract.push(derived);
                            queue.push(derived);
                        }
                    }
                } else {
                    i += 1;
                }
            }
        }

        wmes_to_retract
    }

    /// Retracts one rule-instantiation justification and returns consequences
    /// that no longer have any remaining support.
    pub fn retract_justification(
        &mut self,
        rule_name: &'static str,
        supporting_wmes: &[WmeKey],
    ) -> Vec<WmeKey> {
        let Some(index) = self.justifications.iter().position(|justification| {
            justification.rule_name == rule_name
                && justification.supporting_wmes == supporting_wmes
        }) else {
            return Vec::new();
        };
        let justification = self.justifications.remove(index);
        justification
            .derived_wmes
            .into_iter()
            .filter(|derived| {
                !self
                    .justifications
                    .iter()
                    .any(|remaining| remaining.derived_wmes.contains(derived))
            })
            .collect()
    }

    /// Returns the total number of active justifications tracked by the TMS.
    pub fn active_justifications_count(&self) -> usize {
        self.justifications.len()
    }
}
