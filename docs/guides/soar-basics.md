# Soar in Ten Minutes

This page explains the handful of ideas you need to read and write Soar agents. It describes Soar
itself; Rusty-Soar behaves the same way.

## Working memory: what the agent knows right now

An agent's current knowledge is a graph of small facts called **working memory elements**, or
**WMEs**. Each WME has three parts, written `(identifier ^attribute value)`:

```text
(S1 ^io I1)
(I1 ^input-link I2)
(I2 ^altitude 500)
```

- An **identifier** is a node in the graph, like `S1` or `I2`: a letter and a number.
- An **attribute** names the link, written with `^`.
- A **value** is a number, a string, or another identifier.

Every agent starts with a **top state**, `S1`. Everything else hangs off it, including the input
link (`I2`), where your program puts sensor data, and the output link (`I3`), where the agent
puts commands.

## Rules: if this, then that

Knowledge that doesn't change, the agent's "how to", lives in **rules** (also called
productions). A rule has conditions, an arrow, and actions:

```soar
sp {propose*climb
   (state <s> ^io.input-link.altitude < 1000)
-->
   (<s> ^operator <o> + =)
   (<o> ^name climb)}
```

Words in angle brackets, like `<s>`, are **variables**. A rule **fires** whenever its
conditions match working memory, and keeps matching until they stop being true. Soar matches
every rule at once, in parallel, using an algorithm called **Rete**.

## Operators: one deliberate step at a time

Rules don't act directly; they **propose operators**, the actions an agent can choose between.
The `+` above means "acceptable": `climb` is a candidate. Other rules add **preferences** about
candidates:

| Preference | Meaning |
|---|---|
| `+` | acceptable: this is a candidate |
| `>` / `<` | better or worse than another candidate |
| `=` | indifferent: any of these is fine |
| `= 0.7` | numeric: a value used when choosing, and learned by reinforcement learning |
| `!` | require |
| `-` | reject |

Soar weighs the preferences and **selects one operator**. Then rules that test the selected
operator **apply** it, changing working memory.

## The decision cycle

Soar repeats five phases:

1. **Input**: your program updates the input link.
2. **Propose**: rules fire to propose operators and work out facts (elaborations).
3. **Decide**: Soar selects an operator, or detects an impasse (below).
4. **Apply**: rules apply the selected operator.
5. **Output**: changes on the output link are reported to your program.

One pass is a **decision cycle**. The numbers in a trace (`2: O: O2 (count)`) count them.

## Impasses and substates: thinking about thinking

Sometimes Soar can't decide: two operators tie, preferences conflict, or nothing is proposed.
Instead of failing, Soar creates a **substate**, a new state below the current one, where rules
can work out what to do. The trace shows this with indentation:

```text
     1:    ==>S: S2 (operator tie)
```

When the substate's rules produce a **result** (here, a preference that settles the tie), Soar
makes its decision and removes the substate.

| Impasse | When it happens |
|---|---|
| tie | Several candidates and nothing to pick between them |
| conflict | Preferences contradict each other (`a > b` and `b > a`) |
| constraint failure | Two operators are both required, or a required one is rejected |
| operator no-change | An operator is selected but nothing applies it |
| state no-change | No operators are proposed at all |

## What stays and what goes

A WME made while *applying* an operator stays until something removes it: it has
**o-support** (operator support). A WME made by any other rule disappears as soon as that rule
stops matching: it has **i-support** (instantiation support). This is how Soar keeps
working memory consistent with the world without extra bookkeeping.

## Learning from rewards

Rules with a numeric preference (`= 0.7`) are **RL rules**. With reinforcement learning on,
Soar adjusts their values from rewards the agent receives, so it gradually prefers operators
that worked well. See [Reinforcement Learning](reinforcement-learning.md).
