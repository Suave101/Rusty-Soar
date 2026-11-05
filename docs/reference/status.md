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
| Reinforcement learning | SARSA and Q-learning, eligibility traces, decay modes |
| Decision trace | The `watch 1` trace, in the same format |

## Not yet ported

| Part of Soar | What happens if an agent uses it |
|---|---|
| Chunking (explanation-based learning) | `learn` / `chunk` commands are errors. Results from substates are kept with simplified bookkeeping. |
| Semantic memory | `smem` commands and `@` tests are errors |
| Episodic memory | `epmem` commands are errors |
| Working-memory activation | `wma` commands are errors |
| RL templates and gradient Q-learning | Errors, see [RL Settings](rl-settings.md) |
| Most RHS functions | Unlisted functions are errors, see [RHS Functions](rhs-functions.md) |
| Most CLI commands (`print`, `excise`, `init-soar`, ...) | Errors in scripts |
| SVS (spatial reasoning) | Not planned |
| SML client libraries (Java, Python, ...) | Not planned; Rusty-Soar is used directly from Rust |

## Verification

| Item | Status |
|---|---|
| Comparison with C++ Soar on test agents | Tool ready ([guide](../guides/comparing-with-soar.md)); results to be published |
| Kani proofs | In progress ([status](../guides/formal-verification.md#current-status)) |
