# 4. Inputs and Outputs

Now let's feed the agent some altitudes and see what it commands.

## The control loop

```rust
--8<-- "rusty-soar/crates/rusty-soar-core/examples/getting_started.rs:loop"
```

Each pass through the loop is one **decision cycle**:

1. **Input.** Remove last cycle's altitude and add the new one with `add_input_wme`. A *WME*
   (working memory element) is one fact, here `(I2 ^altitude 500)`.
2. **Run.** `run_decision_cycle` runs Soar's five phases: input, propose, decide, apply and
   output.
3. **Output.** `take_output_events` returns what the agent changed on its output link during
   the cycle.

Symbols such as `altitude` are stored once and referred to by a small handle; `intern_str` and
`intern_int` turn text and numbers into those handles, and `symbol_to_string` turns one back
into text.

## Run it

```bash
cargo run -p rusty-soar-core --example getting_started
```

```text
altitude 500 -> climb
altitude 1200 -> hold
```

!!! question "Why is nothing printed for 800 feet?"

    At 800 feet the agent still wants to `climb`, and `climb` is already on the output link.
    The output phase only reports *changes*, so there is nothing to report. This matches how
    C++ Soar calls its output functions.

## Things to try

- Change the threshold from 1000 to 700 and run it again.
- Add a third operator, `descend`, for altitudes above 2000.
- Print every WME in each output event to see the whole output link.

**Next:** [5. Where to Go Next](next-steps.md)
