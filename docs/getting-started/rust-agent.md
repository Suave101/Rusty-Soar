# 3. Put an Agent in Your Program

The command-line tool is handy for trying agents out, but usually an agent runs inside a larger
program: a simulator, a robot, or an autopilot. In this step you'll create an agent from Rust.

The complete program for steps 3 and 4 is in
`rusty-soar/crates/rusty-soar-core/examples/getting_started.rs`. Run it at any time with:

```bash
cargo run -p rusty-soar-core --example getting_started
```

## The rules

This agent reads an altitude and decides whether to `climb` or `hold`:

```rust
--8<-- "rusty-soar/crates/rusty-soar-core/examples/getting_started.rs:rules"
```

In plain English:

- **`propose*climb`**: if the altitude on the input link is below 1000, suggest `climb`.
- **`propose*hold`**: if it is 1000 or more, suggest `hold`.
- **`apply*command`**: when an operator is selected, put its name on the output link as
  `^command`.
- **`apply*command*remove-old`**: and remove any older, different command.

The **input link** and **output link** are where an agent meets the outside world: your
program writes sensor readings to the input link, and reads commands from the output link.

## Create the agent and load the rules

```rust
--8<-- "rusty-soar/crates/rusty-soar-core/examples/getting_started.rs:create"
```

| Line | What it does |
|---|---|
| `SoarAgent::new()` | Makes an empty agent. |
| `create_top_state()` | Creates the top state `S1` and its input and output links. It returns the state and the input link, which you'll need in the next step. |
| `SoarScript::parse(RULES)` | Reads Soar source text. You can load a `.soar` file the same way by reading it into a string first. |
| `install_soar_script(...)` | Checks the rules the way Soar does and adds them to the agent. |

!!! warning "Unsupported commands are errors"

    If a script contains a command Rusty-Soar doesn't support, `SoarScript::parse` returns an
    error naming it, rather than quietly skipping it. That way an agent can never run
    differently from how its files say it should. See
    [Script Commands](../reference/script-commands.md) for what's supported.

**Next:** [4. Inputs and Outputs](inputs-outputs.md)
