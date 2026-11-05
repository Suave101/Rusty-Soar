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

## Semantic memory

Semantic memory is off by default, as in C++ Soar. Agents use it through the `^smem.command`
and `^smem.result` links of each state.

| Command | Effect |
|---|---|
| `smem --enable` (`-e`, `--on`) | Turns semantic memory on (the same as `smem --set learning on`) |
| `smem --disable` (`-d`, `--off`) | Turns it off |
| `smem --set learning <on\|off>` (`-s`) | The same |
| `smem --set activation-mode <recency\|frequency\|base-level>` | How memories are ranked for queries (default `recency`) |
| `smem --set activate-on-query <on\|off>` | Whether a query's result counts as an access (default `on`) |
| `smem --set activate-on-add <on\|off>` | Whether storing counts as an access (default `off`) |
| `smem --set thresh <n>` | Memories with at least this many attributes are ranked by the memory, not its attributes (default 100) |
| `smem --set base-decay <x>` | Decay rate of base-level activation, above 0 (default 0.5) |
| `smem --set base-update-policy <stable\|naive\|incremental>` | When base-level activation is recomputed: only when a memory is stored or retrieved (`stable`, the default), also for every candidate of a query (`naive`), or also for memories last used a set number of cycles before an access (`incremental`) |
| `smem --set base-incremental-threshes <n>` | Adds `n` to the cycle counts the incremental policy uses, or removes it if it's there (default 10) |
| `smem --set base-unused-age-offset <n>` | Added to the agent's age when estimating the activation of a memory never accessed (default 0) |
| `smem --set base-inhibition <on\|off>` | Whether a recent access lowers base-level activation for a while (default `off`) |
| `smem --add {<clauses>}` (`-a`) | Adds structure to semantic memory, as below |

`smem --add` takes clauses like `(<a> ^name alice ^friend <b>)`. A variable names a new
memory, the same one throughout the command; `@5` names memory 5 and `@name` a memory with
that alias. `^a.b v` makes a new memory for each `.`. Memories that already exist keep their
attributes and gain the new ones. As in C++ Soar, adding doesn't count as an access, and
each memory stored writes `@` to the output.

## Chunking

Chunking is off by default, as in C++ Soar. JSoar 4's `learn --on`, `--off`, `--only` and
`--except` are the same as `chunk always`, `never`, `only` and `except`. JSoar 4 learns with
the chunking from before Soar 9.6, so its learned rules can differ from these.

| Command | Effect |
|---|---|
| `chunk always` | Learns rules in every state |
| `chunk never` | Learns no rules (default) |
| `chunk except` | Learns in every state except those flagged by `dont-learn` |
| `chunk only` | Learns only in states flagged by `force-learn` |
| `chunk bottom-only <on\|off>` | Learns only in the lowest state that makes a result (default `off`) |
| `chunk allow-local-negations <on\|off>` | Whether results that tested the absence of local structure can be learned (default `on`) |
| `chunk add-osk <on\|off>` | Whether rules depend on the preferences that selected an operator (default `on`) |
| `chunk allow-opaque <on\|off>` | Whether rules are learned from results that depend on memory retrievals (default `on`) |
| `chunk max-chunks <n>` | The most rules learned in a decision (default 50) |
| `chunk max-dupes <n>` | The most duplicates from one rule in a decision (default 3) |
| `chunk singleton <id> <attr> <value>` | Treats conditions on one WME of this pattern as one condition; `<id>` and `<value>` are `any`, `constant`, `identifier`, `operator` or `state` |
| `chunk singleton -r <id> <attr> <value>` | Removes the attribute's pattern |
| `chunk automatically-create-singletons <on\|off>` | Whether a string attribute with no pattern counts as a singleton (default `on`) |

## Episodic memory

Episodic memory is off by default, as in C++ Soar. Agents use it through the `^epmem.command`
and `^epmem.result` links of each state.

| Command | Effect |
|---|---|
| `epmem --enable` (`-e`, `--on`) | Turns episodic memory on (the same as `epmem --set learning on`) |
| `epmem --disable` (`-d`, `--off`) | Turns it off |
| `epmem --set learning <on\|off>` (`-s`) | The same |
| `epmem --set phase <output\|selection>` | Stores and answers commands after the output phase (default) or after the decision phase |
| `epmem --set trigger <dc\|output\|none>` | Stores an episode every decision (`dc`, the default), when the output link gains a WME, or never |
| `epmem --set force <remember\|ignore\|off>` | Overrides the trigger once |
| `epmem --set exclusions <attr>` | Adds an attribute not to record, or removes it if it's there (default `epmem` and `smem`) |
| `epmem --set graph-match <on\|off>` | Whether a query checks the cue's structure, not just its values (default `on`) |
| `epmem --set graph-match-ordering <undefined\|dfs\|mcv>` | The order the graph match tries the cue in (default `undefined`) |
| `epmem --set merge <none\|add>` | Accepted; has no effect here |
| `epmem --set balance <0–1>` | Below 1, a query's cue WMEs count by their working-memory activation instead of 1 each (default 1) |

## Working-memory activation

Working-memory activation is off by default, as in C++ Soar. With it on, each o-supported WME
has an activation that rises when rules test it and decays over time, and with forgetting on,
WMEs whose activation falls below the threshold are removed.

| Command | Effect |
|---|---|
| `wma --set activation <on\|off>` (`-s`) | Turns activation on or off |
| `wma --set decay-rate <0–1>` | How fast activation decays (default 0.5) |
| `wma --set decay-thresh <above 0>` | Forgetting removes WMEs whose activation is below minus this (default 2) |
| `wma --set petrov-approx <on\|off>` | Estimates references older than the last 10 (default `off`) |
| `wma --set forgetting <off\|naive\|bsearch\|on>` | Whether, and how, WMEs are forgotten (default `off`) |
| `wma --set forget-wme <all\|lti>` | Forgets any o-supported WME (default), or only all of a long-term identifier's WMEs at once |
| `wma --set fake-forgetting <on\|off>` | Finds WMEs to forget but leaves them (default `off`) |
| `wma --set timers <off\|one>` | Accepted; Rusty-Soar keeps no timers |
| `wma --set max-pow-cache <MB>` | Size of the cache of decay powers (default 10) |
| `wma` | Writes every setting |
| `wma --get <setting>` (`-g`) | Writes one setting. Like C++ Soar, it writes the decay settings negated |
| `wma --history <timetag>` (`-h`) | Writes a WME's reference history |
| `wma --stats` (`-S`) | Writes how many WMEs have been forgotten |

`wm activation ...` is the same command. As in C++ Soar, every setting but `activation` can only
be changed while activation is off.

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

## Waiting

| Command | Effect |
|---|---|
| `soar wait-snc <on\|off>`, `waitsnc <on\|off>` | With `on`, a state no-change waits for something to change instead of making a substate (default `off`). JSoar's `--on`/`--off` (`-e`/`-d`) work too |

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
| `trace --wma` (`-a`) | Writes working-memory activation and forgetting; `trace --wma remove` stops it |
| `trace --epmem`, `--smem`, `--rl` (`-e`, `-s`, `-R`) | Accepted; these traces aren't written |

`w` is short for `trace`, as in C++ Soar.

!!! note

    Trace levels above 1 show only the level-1 decision trace. Trace settings never change how
    an agent behaves.

## Files

| Command | Effect |
|---|---|
| `source <file>` | Loads another file. Accepts `file`, `"file"` and `{file}`. Handled by `rusty-soar-cli`; from Rust, read and combine the files yourself. |
