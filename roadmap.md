### Agent Execution Roadmap: Rusty-Soar Compatibility

#### Phase 1: Instantiation & TMS Correctness (Foundation)

*Goal: Fix production lifecycle and support tracking so rules unmatch and retract correctly.*

* **1.1 Instantiation Identity**
* **Task:** Add unique `InstantiationId` and track active instantiations in `rusty-soar-core`.
* **Criteria:** Rules fire per unique binding, not per evaluation cycle.


* **1.2 Support Classification (i-support vs. o-support)**
* **Task:** Distinguish i-supported actions (derived state, auto-retracted) from o-supported actions (operator application, persistent).
* **Criteria:** Removing a condition automatically retracts its i-supported WMEs while leaving o-supported WMEs intact.


* **1.3 Double-Check & Test Harness**
* **Task:** Create unit tests in `crates/rusty-soar-core/tests/tms.rs` validating firing, retraction, and o-support persistence.



---

#### Phase 2: Decision Cycle & 8-Stage Preference Engine

*Goal: Replace simplified preference deduplication with Soar 8-stage semantics.*

* **2.1 Preference Pool Refactor**
* **Task:** Replace `contains` deduplication in preference handling with dedicated preference structures (acceptable, require, prohibit, reject, better/worse, best/worst, indifferent).
* **Criteria:** Preferences preserve complete context without silent dropping.


* **2.2 Decision Phase Implementation**
* **Task:** Implement exact 8-stage candidate evaluation.
* **Criteria:** Correctly select operators or emit impasses (tie, conflict, constraint-failure) deterministically using fixed ID sorting.


* **2.3 Architectural Operator WMEs & Quiescence**
* **Task:** Automatically create `^operator` on selection. Replace 10-cycle fixed watchdog with true elaboration quiescence detection with a safety limit.



---

#### Phase 3: RETE Engine & Condition Language

*Goal: Expand matching capabilities required by standard Soar programs.*

* **3.1 Condition Negation (`-`) & Relational Predicates**
* **Task:** Add negation nodes and numeric predicates (`>`, `<`, `>=`, `<=`, `<>`) to the RETE builder.


* **3.2 Disjunctions & Attribute Paths**
* **Task:** Support disjunctions `(<a> ^b { <c> <d> })` and dot notation (`^location.x`).


* **3.3 Basic RHS Functions**
* **Task:** Implement `+`, `-`, `*`, `/`, `halt`, `interrupt`, and `write`.



---

#### Phase 4: Impasses, Substates & Goal Dependency Set (GDS)

*Goal: Enable hierarchical problem solving and automated substate removal.*

* **4.1 Substate Construction**
* **Task:** Generate architectural substate WMEs (`^superstate`, `^impasse`, `^type`) when an impasse occurs.


* **4.2 Goal Dependency Set (GDS)**
* **Task:** Link substate WMEs to higher-level dependencies; retract substate immediately when a higher-level condition breaks.


* **4.3 Waterfall Processing**
* **Task:** Enforce top-down elaboration ordering across nested substate stacks.



---

#### Phase 5: Verification & Deterministic Services

*Goal: Lock in `no_std` safety and deterministic memory guarantees.*

* **5.1 Deterministic Memory Stores**
* **Task:** Back SMem/EpMem with deterministic sorted arrays/maps (`BTreeMap` / fixed arena vectors) to eliminate host hash non-determinism.


* **5.2 Kani Proofs**
* **Task:** Write bounded model checking harness for arena allocations, TMS retraction completeness, and quiescence termination.



---

### Execution Strategy for the Agent

To minimize token usage and credit consumption during execution:

1. **Work Single Tasks:** Process one sub-task (e.g., `1.1`) per prompt/turn.
2. **In-Memory Verification:** Run targeted unit tests (`cargo test --test tms`) instead of full rebuilds after every file edit.
3. **Fail-Fast Reporting:** If a task requires unexpected refactoring outside its scope, halt and state the blocker in under 3 sentences before writing code.