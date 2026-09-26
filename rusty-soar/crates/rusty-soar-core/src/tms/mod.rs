//! Truth Maintenance System (TMS) dependency tracking for automated I-support retractions.

use alloc::vec;
use alloc::vec::Vec;
use crate::wm::WmeKey;

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
                        if !wmes_to_retract.contains(&derived) {
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

    /// Returns the total number of active justifications tracked by the TMS.
    pub fn active_justifications_count(&self) -> usize {
        self.justifications.len()
    }
}