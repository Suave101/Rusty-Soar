# What's Ported

Rusty-Soar ports the C++ Soar kernel piece by piece. This page lists what is there and what is
not yet. "Ported" means it follows the C++ Soar algorithm; any known differences are listed in
[Differences from C++ Soar](differences.md).

## Ported

| Part of Soar | Notes |
|---|---|
| Rule parser and checks | Soar's lexer and parser; rules are checked as Soar checks them |
| Rete matching | Positive, negated and negated-conjunction conditions; the same matches as Soar |
| Preferences and operator selection | All preference types; softmax, epsilon-greedy, Boltzmann, first and last; Soar's random number generator |
| Decision cycle | Input, propose, decide, apply and output phases |
| Impasses and substates | Tie, conflict, constraint failure, operator no-change and state no-change |
| O-support and i-support | As Soar calculates them |
| Goal dependency sets | Substates are removed when the facts they depend on change |
| Garbage collection | Unreachable identifiers are removed as in Soar |
| Input and output links | Input WMEs, and output reports (added, modified, removed) |
| Reinforcement learning | SARSA, Q-learning and both GQ(λ) policies, eligibility traces, decay modes, `:template` rules |
| Semantic memory | `store`, `store-new`, `retrieve` and `query` commands (with `neg-query`, `math-query`, `prohibit` and `depth`); recency, frequency and base-level activation, with the stable, naive and incremental update policies; `smem --add`; `@` link tests in rules and the `(@ <x>)` function |
| Episodic memory | Storing an episode each decision (or on output), `retrieve`, `next`, `previous` and `query` commands (with `neg-query`, `before`, `after` and `prohibit`), graph matching |
| Chunking | Learns a rule from a substate result: backtracing, identities and variablization, constraints, negated conditions, rule repair, duplicate detection and refraction. See the gaps below. |
| Working-memory activation | Activation from rule matches and preferences, decay histories, Petrov's approximation, forgetting (`naive`, `bsearch` and `on`, all WMEs or LTIs only), and the `wma` command. See the differences below. |
| Decision trace | The `watch 1` trace, in the same format |

## Not yet ported

| Part of Soar | What happens if an agent uses it |
|---|---|
| Chunking: other parts | `explain` is not ported. `chunk` settings not listed in [script commands](script-commands.md) are errors. |
| Semantic memory: spreading activation | The spreading settings are errors |
| Semantic memory: other commands | `smem` subcommands other than `--set`, `--enable`, `--disable` and `--add` (`--print`, `--query`, `--remove`, ...) are errors |
| Episodic memory: other parts | File databases, and `epmem` subcommands other than `--set`, `--enable` and `--disable`, are errors |
| A few RHS functions | `accept`, `timestamp`, `deep-copy`, `from-st-xml`, `exec`, `cmd` and `interrupt` are errors, see [RHS Functions](rhs-functions.md) |
| Most CLI commands (`print`, `excise`, `init-soar`, ...) | Errors in scripts |
| SVS (spatial reasoning) | Not planned |
| SML client libraries (Java, Python, ...) | Not planned; Rusty-Soar is used directly from Rust |

## Verification

| Item | Status |
|---|---|
| Comparison with C++ Soar on test agents | Tool ready ([guide](../guides/comparing-with-soar.md)); results to be published |
| Kani proofs | In progress ([status](../guides/formal-verification.md#current-status)) |
