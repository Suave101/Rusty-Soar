# Formal Verification

Testing checks the cases you think of. **Formal verification** checks every case, within stated
limits. Rusty-Soar uses [Kani](https://model-checking.github.io/kani/), a model checker for Rust,
to prove properties of the kernel and, eventually, of agents.

!!! warning "Work in progress"

    The proof suite is being built. Proofs about the memory pools, the symbol table, the
    Rete network's matching (positive and negated conditions) and a first whole decision cycle
    verify. Proofs over impasses and substates, and against AGREE contracts, are not written
    yet. This page explains how it works and where it stands.

## What a Kani proof looks like

A proof harness is a Rust function marked `#[kani::proof]`. Values made with `kani::any()` stand
for *every possible value*, so one harness covers all of them:

```rust
#[kani::proof]
#[kani::unwind(5)]
fn negated_condition_matches_correctly() {
    // A rule: (<s> ^a <v>) -(<s> ^b <v>)
    // ... build the network ...
    let has_a: bool = kani::any();
    let has_b: bool = kani::any();
    // add the WMEs that are present, then check the rule matches
    // exactly when the definition of the rule says it should
}
```

Kani explores all combinations of `has_a` and `has_b` and either proves the assertions or gives
a counterexample.

`#[kani::unwind(N)]` limits loops to `N` iterations. If any loop could need more, Kani reports
it instead of giving a wrong "verified".

## Running the proofs

Install Kani once:

```bash
cargo install --locked kani-verifier
cargo kani setup
```

Then, from `rusty-soar/crates/formal-soar-verify`:

```bash
cargo kani --harness <name> --exact
```

Proofs with several symbolic choices run faster **path by path**: Kani checks each combination
of choices separately instead of merging them into one large formula.

```bash
cargo kani --harness verify_rete_negated_condition --exact --cbmc-args --paths lifo
```

The decision-cycle proofs run without **assertion reachability checks**. By default Kani asks
the solver separately whether each assertion can be reached, which for a whole agent means
thousands of solver calls and about an hour. Each of these proofs instead ends with a
`kani::cover!`, which shows the end of the cycle is reached, so the proof is not passing
vacuously:

```bash
cargo kani --harness decision_cycle::verify_decision_cycle_selects_proposal --exact --no-assertion-reach-checks
```

## Why the kernel is shaped the way it is

Making Soar provable has meant writing parts of the kernel differently from a typical Rust
program, while keeping the same algorithm:

- **Fixed-size memory.** The Rete network and working memory allocate all their storage when
  created, and never grow. If a pool fills up, the agent stops with an error instead of running
  on with missing matches.
- **No recursive data in proofs.** Kani cannot bound Rust's automatically generated cleanup code
  for types that contain themselves. Rule conditions therefore have a flat form
  (`FlatCondition`) that proofs use.
- **Fixed-size arrays for proofs.** The Rete's pools can live on the heap (`HeapSlots`, for
  agents) or in arrays inside the network (`InlineSlots<N>`, for proofs). Kani tracks typed
  arrays far more precisely than heap memory. Both run the same code.
- **Explicit enum tags.** Rust usually hides an enum's tag in spare values of one of its
  fields. CBMC cannot always read such a tag back as a known value, and then it explores
  every branch on it both ways, so loops can no longer be bounded. The Rete's enums with
  data are marked `#[repr(u8)]`, which gives them an ordinary tag byte.
- **No enums inside enums.** CBMC keeps the data of an enum variant as raw bytes, and an enum
  stored there loses its known tag, even when copied out. Values the engine branches on in
  loops are therefore kept in structs with a plain "kind" field instead. Rule actions are
  stored this way (`FlatAction`), and the symbol table keeps a flat record of every symbol
  alongside its data.
- **Rule tests off the heap.** In heap memory, CBMC cannot tell which variant of a rule test
  (`ReteTest`) a value belongs to. `FlatCondition` therefore borrows its tests as a slice,
  so a proof can keep them in an ordinary array.
- **Any random choice.** Soar breaks ties between indifferent operators at random. In proofs,
  the random number generator is replaced (with Kani stubs) by one that can return any value
  in range, so a proof covers every choice the agent could make, not just one seed's.
- **CBMC settings.** By default the CBMC solver under Kani only tracks arrays of up to 64
  elements in detail. The Rete's pools are larger, so the project raises that limit in
  `formal-soar-verify/Cargo.toml`.

## Current status

| Proof | Status |
|---|---|
| Memory pool insert and remove | Verifies in seconds |
| Symbol table | Verifies in seconds |
| Building an empty Rete network | Verifies in seconds |
| Adding a rule to the Rete | Verifies in under a minute |
| Matching a single condition, WME present or not | Verifies in about a minute |
| Matching a negated condition: every combination of adding and removing the WMEs | Verifies in about 6 minutes, path by path |
| A decision cycle: one rule proposes an operator, which is selected | Verifies in about 6 minutes |
| A decision cycle: two rules propose indifferent operators, either or both WMEs present; any random choice selects a proposed operator | Written; runs out of memory on an 8 GB machine (CBMC needs more than 7 GB) |
| A decision cycle: one rule's WME present or not; it is selected, or a state no-change impasse creates a substate | Written; runs out of memory on an 8 GB machine (CBMC needs more than 7 GB) |
| An agent against an AGREE contract | Not started |

The Rete proofs use symbols made directly (`SymbolId(0)`, `SymbolId(1)`, ...) because the Rete
compares symbols only for equality. The decision-cycle proofs use a real symbol table: its
lookups and the cycle's questions about symbols read flat records, which Kani follows exactly.

An impasse adds about 14 WMEs for the substate. An inline slot family gives every pool the same
number of slots, and with 32 slots in all of the Rete's pools the agent is too large for Kani to
build in under an hour. A proof can instead define a `Layout`, which picks a slot family for each
pool, and give only the WME pool room for the substate.
