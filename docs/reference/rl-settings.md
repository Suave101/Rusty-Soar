# RL Settings

Set with `rl --set <setting> <value>` in a script, or `agent.set_rl(RlSetting::...)` from Rust.
Defaults are C++ Soar's.

| Setting | Values | Default | Meaning |
|---|---|---|---|
| `learning` | `on`, `off` | `off` | Turns learning on or off. Turning it off clears pending reward and traces. |
| `learning-policy` | `sarsa`, `q-learning`, `on-policy-gq-lambda`, `off-policy-gq-lambda` | `sarsa` | Learn toward the operator chosen (SARSA, on-policy GQ(λ)) or the best one (Q-learning, off-policy GQ(λ)) |
| `learning-rate` | 0 to 1 | 0.3 | Alpha: how far each update moves a value |
| `discount-rate` | 0 to 1 | 0.9 | Gamma: how much each step into the future counts |
| `eligibility-trace-decay-rate` | 0 to 1 | 0 | Lambda: how quickly credit for earlier choices fades |
| `eligibility-trace-tolerance` | above 0 | 0.001 | Traces smaller than this are dropped |
| `temporal-extension` | `on`, `off` | `on` | Carry learning across decisions where no RL rule applied |
| `temporal-discount` | `on`, `off` | `on` | Discount reward for the decisions skipped over |
| `hrl-discount` | `on`, `off` | `off` | Discount reward in states above the bottom one, for hierarchical RL |
| `decay-mode` | `normal`, `exp`, `log`, `delta-bar-delta` | `normal` | How the learning rate shrinks as a rule is updated |
| `step-size-parameter` | 0 to 1 | 1.0 | Eta: how far GQ(λ) moves each rule's gradient weight |
| `meta-learning-rate` | 0 to 1 | 0.1 | Used by `delta-bar-delta` |

## Decay modes

| Mode | Learning rate for a rule updated *n* times |
|---|---|
| `normal` | `learning-rate` |
| `exp` | 1 / (*n* + 1) |
| `log` | 1 / (ln(*n* + 1) + 1) |
| `delta-bar-delta` | adapted per rule, using `meta-learning-rate` |

## Not supported

These C++ Soar settings are errors if used: `chunk-stop`, `meta`, `update-log-path`,
`apoptosis`, `apoptosis-decay`, `apoptosis-thresh` and `trace`.

## GQ(λ)

The two GQ(λ) policies update as SARSA and Q-learning do, and also keep a gradient weight for
each RL rule. After each update, every numeric preference then in the state's operator slot
lowers its rule's expected future reward by α·γ·(1 − λ) times the traced weights, and the
previous operator's rules lower their weights. Off-policy GQ(λ) also scales the traces by how
likely the greedy policy was to make the choice that was made, relative to the exploration
policy (C++ Soar's `rho`); under `first` or `last` selection that is 1.
