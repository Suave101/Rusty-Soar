# Script Commands

Besides `sp` rules, a `.soar` file may contain these commands. They follow C++ Soar's syntax and
check their values the same way. They take effect in the order they appear, among the rules.

Any other command makes loading fail with `Unsupported command: ...`.

## Reinforcement learning

```soar
rl --set <setting> <value>
rl -s <setting> <value>
```

See [RL Settings](rl-settings.md) for every setting and the values it accepts.

## Choosing between equal operators

| Command | Effect |
|---|---|
| `decide indifferent-selection --softmax` (`-x`) | Choose with probability proportional to value (default) |
| `decide indifferent-selection --epsilon-greedy` (`-g`) | Usually the best; with probability epsilon, a random one |
| `decide indifferent-selection --boltzmann` (`-b`) | Boltzmann (softmax over value / temperature) |
| `decide indifferent-selection --first` (`-f`) | Always the first candidate |
| `decide indifferent-selection --last` (`-l`) | Always the last candidate |
| `decide indifferent-selection --epsilon <0–1>` (`-e`) | Sets epsilon (default 0.1) |
| `decide indifferent-selection --temperature <above 0>` (`-t`) | Sets temperature (default 25) |
| `decide numeric-indifferent-mode --sum` (`-s`) | Add up numeric preferences (default) |
| `decide numeric-indifferent-mode --avg` (`-a`, `--average`) | Average them |

C++ Soar's shorter forms work too: `indifferent-selection ...`, `inds ...` and
`numeric-indifferent-mode ...`.

## Random seed

| Command | Effect |
|---|---|
| `srand <seed>` | Seeds the random number generator |
| `decide srand <seed>`, `decide set-random-seed <seed>` | The same |

!!! note

    In C++ Soar, `srand` without a seed seeds from the clock. That can't be reproduced, so
    Rusty-Soar requires a seed. Without any `srand`, the seed is 0.

## Output

| Command | Effect |
|---|---|
| `echo <words>` | Writes the words and a newline to the agent's output |
| `echo -n <words>` (`--nonewline`) | The same, without the newline |
| `watch <level>`, `trace <level>`, `trace --level <level>` | `0` turns the decision trace off; any higher level turns it on |

!!! note

    Trace levels above 1 show only the level-1 decision trace. Trace settings never change how
    an agent behaves.

## Files

| Command | Effect |
|---|---|
| `source <file>` | Loads another file. Accepts `file`, `"file"` and `{file}`. Handled by `rusty-soar-cli`; from Rust, read and combine the files yourself. |
