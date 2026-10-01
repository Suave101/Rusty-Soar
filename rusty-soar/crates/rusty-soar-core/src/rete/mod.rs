use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::vec::Vec;

/// Symbol field selector inside a Working Memory Element (WME) triple (Id ^Attr Val).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Identifier field of the WME triple.
    Id,
    /// Attribute field of the WME triple.
    Attr,
    /// Value field of the WME triple.
    Val,
}

/// Relational predicate for a constant value test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// Exact equality.
    Equal,
    /// Inequality.
    NotEqual,
    /// Numeric less-than comparison.
    Less,
    /// Numeric less-than-or-equal comparison.
    LessOrEqual,
    /// Numeric greater-than comparison.
    Greater,
    /// Numeric greater-than-or-equal comparison.
    GreaterOrEqual,
}

impl Field {
    /// Extracts the selected symbol handle from a WME triple.
    pub fn extract(&self, triple: &(SymbolId, SymbolId, SymbolId)) -> SymbolId {
        match self {
            Field::Id => triple.0,
            Field::Attr => triple.1,
            Field::Val => triple.2,
        }
    }
}

/// Variable join constraint across condition boundaries in the Beta Network.
#[derive(Debug, Clone)]
pub struct VariableBinding {
    /// Index of the WME in the historic token chain to compare against.
    pub token_wme_index: usize,
    /// Field in the historic token WME to test.
    pub token_field: Field,
    /// Field in the incoming Alpha WME to test.
    pub wme_field: Field,
}

/// Constant field filter test executed inside Alpha Memory nodes.
#[derive(Debug, Clone, Default)]
pub struct AlphaTest {
    /// Optional identifier requirement.
    pub id: Option<SymbolId>,
    /// Optional attribute requirement.
    pub attr: Option<SymbolId>,
    /// Optional value requirement.
    pub val: Option<SymbolId>,
}

impl AlphaTest {
    /// Tests whether a WME triple matches the alpha filter criteria.
    pub fn matches(&self, s: SymbolId, a: SymbolId, v: SymbolId) -> bool {
        self.matches_with_numeric(s, a, v, &[], Relation::Equal, &[])
    }

    /// Tests a WME using relation-aware numeric symbol metadata.
    pub fn matches_with_numeric(
        &self,
        s: SymbolId,
        a: SymbolId,
        v: SymbolId,
        numeric_values: &[(SymbolId, f64)],
        relation: Relation,
        alternatives: &[SymbolId],
    ) -> bool {
        if let Some(id) = self.id {
            if id != s {
                return false;
            }
        }
        if let Some(attr) = self.attr {
            if attr != a {
                return false;
            }
        }
        if let Some(expected) = self.val {
            if !alternatives.is_empty() && relation == Relation::Equal {
                return alternatives.contains(&v);
            }
            match relation {
                Relation::Equal if expected != v => return false,
                Relation::NotEqual if expected == v => return false,
                Relation::Less
                | Relation::LessOrEqual
                | Relation::Greater
                | Relation::GreaterOrEqual => {
                    let Some(expected) = numeric_values
                        .iter()
                        .find(|(symbol, _)| *symbol == expected)
                        .map(|(_, value)| *value)
                    else {
                        return false;
                    };
                    let Some(actual) = numeric_values
                        .iter()
                        .find(|(symbol, _)| *symbol == v)
                        .map(|(_, value)| *value)
                    else {
                        return false;
                    };
                    let matches = match relation {
                        Relation::Less => actual < expected,
                        Relation::LessOrEqual => actual <= expected,
                        Relation::Greater => actual > expected,
                        Relation::GreaterOrEqual => actual >= expected,
                        Relation::Equal | Relation::NotEqual => unreachable!(),
                    };
                    if !matches {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }
}

/// Recorded Working Memory entry with key tracking for clean retraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmeRecord {
    /// Arena key handle for generational integrity.
    pub key: WmeKey,
    /// Raw symbol identifier tuple (Id, Attr, Val).
    pub triple: (SymbolId, SymbolId, SymbolId),
}

/// Partial match token tracking historic WME bindings through Beta Memory nodes.
#[derive(Debug, Clone)]
pub struct Token {
    /// Sequence of matched WME records forming the partial rule match path.
    pub wmes: Vec<WmeRecord>,
}

/// Index handle referencing an Alpha Memory node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlphaMemoryId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a Beta Memory node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BetaMemoryId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a Join Node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JoinNodeId(
    /// Raw arena index.
    pub usize,
);

/// Index handle referencing a registered Production rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductionId(
    /// Raw arena index.
    pub usize,
);

/// Storage container for WMEs sharing a common constant Alpha filter test.
#[derive(Debug, Clone, Default)]
pub struct AlphaMemory {
    /// Constant condition test filter.
    pub test: AlphaTest,
    /// Cached WME records passing this alpha filter.
    pub wmes: Vec<WmeRecord>,
    /// Dependent join nodes attached to this alpha memory.
    pub successors: Vec<JoinNodeId>,
}

/// Storage container for partial match tokens in the Beta Network.
#[derive(Debug, Clone, Default)]
pub struct BetaMemory {
    /// Tokens representing partial rule matches up to this node.
    pub tokens: Vec<Token>,
    /// Dependent join nodes listening to token additions.
    pub successors: Vec<JoinNodeId>,
}

/// Two-input join node evaluating variable binding constraints between Beta and Alpha inputs.
#[derive(Debug, Clone)]
pub struct JoinNode {
    /// Handle to the right-input Alpha Memory.
    pub alpha_memory: AlphaMemoryId,
    /// Handle to the left-input parent Beta Memory (if any).
    pub parent_beta: Option<BetaMemoryId>,
    /// Cross-condition variable binding constraints to test.
    pub bindings: Vec<VariableBinding>,
    /// Handle to the downstream child Beta Memory (if any).
    pub child_beta: Option<BetaMemoryId>,
    /// Terminal production rule handle satisfied by this join path.
    pub production: Option<ProductionId>,
}

/// Active rule instantiation generated when a full RETE match path is satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstantiationId(pub usize);

/// Active rule instantiation generated when a full RETE match path is satisfied.
#[derive(Debug, Clone)]
pub struct Instantiation {
    /// Stable identity for this rule/binding while it remains matched.
    pub id: InstantiationId,
    /// Production rule identifier label.
    pub rule_name: &'static str,
    /// Complete collection of WME records satisfying the production.
    pub matched_wmes: Vec<WmeRecord>,
}

/// Index-based, deterministic RETE match network engine.
#[derive(Debug, Default)]
pub struct ReteNetwork {
    /// Arena storage for Alpha Memories.
    pub alpha_memories: Vec<AlphaMemory>,
    /// Arena storage for Beta Memories.
    pub beta_memories: Vec<BetaMemory>,
    /// Arena storage for Join Nodes.
    pub join_nodes: Vec<JoinNode>,
    /// List of registered production rule descriptors.
    pub productions: Vec<(&'static str, ProductionId)>,
    /// Active rule instantiations ready for execution.
    pub activations: Vec<Instantiation>,
    /// Numeric values associated with interned symbols for relational tests.
    pub numeric_values: Vec<(SymbolId, f64)>,
    /// Relation metadata for alpha patterns, kept outside the public alpha test shape.
    pub relations: Vec<((Option<SymbolId>, Option<SymbolId>, Option<SymbolId>), Relation)>,
    /// Alternative value symbols for disjunctive alpha patterns.
    pub alternatives: Vec<((Option<SymbolId>, Option<SymbolId>), Vec<SymbolId>)>,
    /// Instantiations whose bindings currently remain matched.
    pub active_instantiations: Vec<Instantiation>,
    /// Monotonically increasing instantiation identity source.
    next_instantiation_id: usize,
}

impl ReteNetwork {
    /// Creates a new, empty RETE match network.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a deterministic numeric interpretation for a symbol.
    pub fn register_numeric_value(&mut self, symbol: SymbolId, value: f64) {
        if let Some((_, existing)) = self
            .numeric_values
            .iter_mut()
            .find(|(existing, _)| *existing == symbol)
        {
            *existing = value;
        } else {
            self.numeric_values.push((symbol, value));
        }
    }

    /// Registers a relation for an existing alpha pattern.
    pub fn register_relation(&mut self, test: &AlphaTest, relation: Relation) {
        let key = (test.id, test.attr, test.val);
        if let Some((_, existing)) = self.relations.iter_mut().find(|(candidate, _)| *candidate == key)
        {
            *existing = relation;
        } else {
            self.relations.push((key, relation));
        }
    }

    /// Registers alternative value symbols for an alpha pattern.
    pub fn register_alternatives(&mut self, test: &AlphaTest, alternatives: Vec<SymbolId>) {
        let key = (test.id, test.attr);
        if let Some((_, existing)) = self.alternatives.iter_mut().find(|(candidate, _)| *candidate == key) {
            *existing = alternatives;
        } else {
            self.alternatives.push((key, alternatives));
        }
    }

    fn alternatives_for(&self, test: &AlphaTest) -> &[SymbolId] {
        self.alternatives
            .iter()
            .find(|(candidate, _)| *candidate == (test.id, test.attr))
            .map(|(_, alternatives)| alternatives.as_slice())
            .unwrap_or(&[])
    }

    fn relation_for(&self, test: &AlphaTest) -> Relation {
        self.relations
            .iter()
            .find(|(candidate, _)| *candidate == (test.id, test.attr, test.val))
            .map(|(_, relation)| *relation)
            .unwrap_or(Relation::Equal)
    }

    fn binding_is_active(&self, rule_name: &'static str, matched_wmes: &[WmeRecord]) -> bool {
        self.active_instantiations.iter().any(|instantiation| {
            instantiation.rule_name == rule_name
                && instantiation.matched_wmes.len() == matched_wmes.len()
                && instantiation
                    .matched_wmes
                    .iter()
                    .zip(matched_wmes)
                    .all(|(left, right)| left.key == right.key)
        })
    }

    /// Adds a production rule to the RETE match graph.
    pub fn add_rule(
        &mut self,
        name: &'static str,
        conditions: Vec<(AlphaTest, Vec<VariableBinding>)>,
    ) -> ProductionId {
        let prod_id = ProductionId(self.productions.len());
        self.productions.push((name, prod_id));

        let mut current_beta: Option<BetaMemoryId> = None;
        let num_conds = conditions.len();

        for (idx, (alpha_test, bindings)) in conditions.into_iter().enumerate() {
            let alpha_id = self.get_or_create_alpha_memory(alpha_test);
            let join_id = JoinNodeId(self.join_nodes.len());
            let is_last = idx == num_conds - 1;

            let child_beta = if !is_last {
                let b_id = BetaMemoryId(self.beta_memories.len());
                self.beta_memories.push(BetaMemory::default());
                Some(b_id)
            } else {
                None
            };

            let join_node = JoinNode {
                alpha_memory: alpha_id,
                parent_beta: current_beta,
                bindings,
                child_beta,
                production: if is_last { Some(prod_id) } else { None },
            };

            self.join_nodes.push(join_node);
            self.alpha_memories[alpha_id.0].successors.push(join_id);

            if let Some(p_beta) = current_beta {
                self.beta_memories[p_beta.0].successors.push(join_id);
            }

            current_beta = child_beta;
        }

        prod_id
    }

    /// Ingests a new WME into the RETE network.
    pub fn add_wme(&mut self, key: WmeKey, s: SymbolId, a: SymbolId, v: SymbolId) {
        let record = WmeRecord {
            key,
            triple: (s, a, v),
        };

        for alpha_idx in 0..self.alpha_memories.len() {
            if self.alpha_memories[alpha_idx]
                .test
                .matches_with_numeric(
                    s,
                    a,
                    v,
                    &self.numeric_values,
                    self.relation_for(&self.alpha_memories[alpha_idx].test),
                    self.alternatives_for(&self.alpha_memories[alpha_idx].test),
                )
            {
                self.alpha_memories[alpha_idx].wmes.push(record.clone());

                let successors = self.alpha_memories[alpha_idx].successors.clone();
                for join_id in successors {
                    self.right_activate_join(join_id, &record);
                }
            }
        }
    }

    /// Retracts a WME and cleans up dependent tokens and activations.
    pub fn remove_wme(&mut self, key: WmeKey) {
        for alpha in &mut self.alpha_memories {
            alpha.wmes.retain(|w| w.key != key);
        }
        for beta in &mut self.beta_memories {
            beta.tokens.retain(|t| !t.wmes.iter().any(|w| w.key == key));
        }
        self.activations
            .retain(|inst| !inst.matched_wmes.iter().any(|w| w.key == key));
        self.active_instantiations
            .retain(|inst| !inst.matched_wmes.iter().any(|w| w.key == key));
    }

    fn get_or_create_alpha_memory(&mut self, test: AlphaTest) -> AlphaMemoryId {
        if let Some(idx) = self.alpha_memories.iter().position(|a| {
            a.test.id == test.id
                && a.test.attr == test.attr
                && a.test.val == test.val
                && self.relation_for(&a.test) == self.relation_for(&test)
        }) {
            AlphaMemoryId(idx)
        } else {
            let id = AlphaMemoryId(self.alpha_memories.len());
            self.alpha_memories.push(AlphaMemory {
                test,
                wmes: Vec::new(),
                successors: Vec::new(),
            });
            id
        }
    }

    /// Right activation: Driven by new WMEs arriving from Alpha Memory.
    fn right_activate_join(&mut self, join_id: JoinNodeId, wme: &WmeRecord) {
        let join = self.join_nodes[join_id.0].clone();

        match join.parent_beta {
            None => {
                let token = Token {
                    wmes: alloc::vec![wme.clone()],
                };
                self.propagate_token(&join, token);
            }
            Some(parent_beta_id) => {
                let parent_tokens = self.beta_memories[parent_beta_id.0].tokens.clone();
                for token in parent_tokens {
                    if self.evaluate_bindings(&join.bindings, &token, wme) {
                        let mut new_wmes = token.wmes.clone();
                        new_wmes.push(wme.clone());
                        self.propagate_token(&join, Token { wmes: new_wmes });
                    }
                }
            }
        }
    }

    fn propagate_token(&mut self, join: &JoinNode, token: Token) {
        if let Some(beta_id) = join.child_beta {
            self.beta_memories[beta_id.0].tokens.push(token.clone());
            let successors = self.beta_memories[beta_id.0].successors.clone();
            for succ_id in successors {
                self.left_activate_join(succ_id, token.clone());
            }
        }

        if let Some(prod_id) = join.production {
            let rule_name = self.productions[prod_id.0].0;
            if !self.binding_is_active(rule_name, &token.wmes) {
                let instantiation = Instantiation {
                    id: InstantiationId(self.next_instantiation_id),
                    rule_name,
                    matched_wmes: token.wmes,
                };
                self.next_instantiation_id += 1;
                self.active_instantiations.push(instantiation.clone());
                self.activations.push(instantiation);
            }
        }
    }

    /// Left activation: Driven by new tokens arriving from Beta Memory.
    fn left_activate_join(&mut self, join_id: JoinNodeId, token: Token) {
        let join = self.join_nodes[join_id.0].clone();
        let alpha_wmes = self.alpha_memories[join.alpha_memory.0].wmes.clone();

        for wme in alpha_wmes {
            if self.evaluate_bindings(&join.bindings, &token, &wme) {
                let mut new_wmes = token.wmes.clone();
                new_wmes.push(wme.clone());
                self.propagate_token(&join, Token { wmes: new_wmes });
            }
        }
    }

    fn evaluate_bindings(
        &self,
        bindings: &[VariableBinding],
        token: &Token,
        wme: &WmeRecord,
    ) -> bool {
        for binding in bindings {
            if let Some(historic_wme) = token.wmes.get(binding.token_wme_index) {
                let left_val = binding.token_field.extract(&historic_wme.triple);
                let right_val = binding.wme_field.extract(&wme.triple);
                if left_val != right_val {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }
}
