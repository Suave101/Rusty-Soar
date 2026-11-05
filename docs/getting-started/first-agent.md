# 2. Run Your First Agent

Rusty-Soar comes with a few small agents in the `differential/agents` folder. Let's run the
simplest one, which counts to five.

## Look at the rules

Open `differential/agents/count.soar`. A Soar agent is a set of **rules**, each written with
`sp`. Here are two of them:

```soar
--8<-- "differential/agents/count.soar:15:26"
```

- `propose*count` says: *while the count is below 5, suggest an operator named `count`.*
- `apply*count` says: *when the `count` operator is selected, replace the count with count + 1
  and print it.*

An **operator** is an action the agent can choose to take. Soar decides which operator to apply,
one at a time. [Soar in Ten Minutes](../guides/soar-basics.md) explains this properly; for now
that's enough.

## Run it

From the `rusty-soar` folder:

```bash
cargo run -p rusty-soar-cli -- trace ../differential/agents/count.soar --decisions 10
```

You should see:

```text
     0: ==>S: S1
     1:    O: O1 (init)
     2:    O: O2 (count)
count 1
     3:    O: O3 (count)
count 2
     4:    O: O4 (count)
count 3
     5:    O: O5 (count)
count 4
     6:    O: O6 (count)
count 5
     7:    O: O7 (done)
done 3.500000 1.500000
```

## Read the trace

Each numbered line is one **decision**: the moment Soar picks what to do next.

| Part of the line | Meaning |
|---|---|
| `2:` | The decision number. |
| `O: O2 (count)` | Soar selected operator `O2`, whose name is `count`. |
| `==>S: S1` | A state. `S1` is the top state, which exists before anything else happens. |

The lines in between, like `count 1`, are what the agent printed with its `write` action.

The agent stopped after decision 7 because the `done` rule calls `(halt)`, even though we
allowed 10 decisions.

!!! note "This is exactly what C++ Soar prints"

    The trace uses the same format as C++ Soar's default `watch 1` output. That is how
    Rusty-Soar is checked against Soar: run the same agent in both and compare the traces. See [Comparing with C++ Soar](../guides/comparing-with-soar.md).

## Try an agent that gets stuck

Run the `impasses` agent:

```bash
cargo run -p rusty-soar-cli -- trace ../differential/agents/impasses.soar --decisions 5
```

```text
     0: ==>S: S1
     1:    ==>S: S2 (operator tie)
     2:    O: O1 (b)
applied b
     3:    O: O3 (c)
     4:    ==>S: S3 (operator no-change)
     5:       ==>S: S4 (state no-change)
```

At decision 1 the agent can't choose between two operators, so Soar creates a **substate**,
`S2`, to work out which is better (the indented line). This is called an **impasse**, and it is
how Soar agents reason about their own decisions. Here a rule in the substate prefers `b`, so
decision 2 selects it.

**Next:** [3. Put an Agent in Your Program](rust-agent.md)
