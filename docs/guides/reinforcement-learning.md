# Reinforcement Learning

Soar's reinforcement learning (RL) lets an agent improve its choices from rewards. Rusty-Soar
ports C++ Soar's RL: SARSA and Q-learning, with eligibility traces.

## How it works, in short

1. **RL rules** give operators numeric preferences. Any rule whose *only* action is a numeric
   preference is an RL rule:

    ```soar
    sp {rl*left
       (state <s> ^operator <o> +)
       (<o> ^name left)
    -->
       (<s> ^operator <o> = 0)}
    ```

2. When choosing between operators, Soar adds up each candidate's numeric preferences and the
   exploration policy picks one, usually favoring higher values.
3. The agent receives **reward** by putting it on the state's reward link:

    ```soar
    sp {reward*left
       (state <s> ^reward-link <rl> ^last-arm left)
    -->
       (<rl> ^reward <w>)
       (<w> ^value 1)}
    ```

4. At the next decision, Soar moves the values of the RL rules that supported the previous
   operator toward the reward plus the discounted value of what came next.

## Turn learning on

In the agent's `.soar` file:

```soar
rl --set learning on
indifferent-selection --epsilon-greedy
```

Or from Rust:

```rust
use rusty_soar_core::preference::ExplorationPolicy;
use rusty_soar_core::rl::RlSetting;

agent.set_rl(RlSetting::Learning(true));
agent.exploration.policy = ExplorationPolicy::EpsilonGreedy;
```

!!! tip "Use `set_rl` rather than editing `rl.params` directly"

    Turning learning **off** also clears the agent's learning history (traces and pending
    reward), as C++ Soar does. `set_rl` does that for you.

## A complete example

`rusty-soar/crates/rusty-soar-core/examples/rl_demo.rs` is a two-armed bandit: each round the
agent picks `left` (reward +1) or `right` (reward −1).

```bash
cargo run -p rusty-soar-core --example rl_demo
```

```text
learned value of left:  8.0533 (96 updates)
learned value of right: 1.2524 (2 updates)
```

The value of `left` heads toward 10, the total discounted reward of choosing it forever
(1 / (1 − 0.9)), and the agent quickly stops trying `right`.

## Reading learned values

```rust
agent.rl.rule_value("rl*left");          // Some(8.05...)
agent.rl.rule_update_count("rl*left");   // Some(96.0)
```

## Settings

All settings and their defaults are in [RL Settings](../reference/rl-settings.md). The ones you
are most likely to change:

| Setting | Default | What it does |
|---|---|---|
| `learning` | off | Turns learning on or off |
| `learning-policy` | `sarsa` | `sarsa` learns from what the agent did; `q-learning` from the best option |
| `learning-rate` | 0.3 | How far each update moves a value |
| `discount-rate` | 0.9 | How much future reward counts |
| `eligibility-trace-decay-rate` | 0 | How far back credit is spread |

## Not yet supported

- the gradient Q-learning policies (`on-policy-gq-lambda`, `off-policy-gq-lambda`)
- RL `:template` rules
- RL trace output and statistics

Using any of these in a script is an error, not silently ignored.
