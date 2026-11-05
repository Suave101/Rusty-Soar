# Command-Line Tool

`rusty-soar-cli` runs agents and checks them against contracts. Run it through `cargo` from the
`rusty-soar` folder:

```bash
cargo run -p rusty-soar-cli -- <arguments>
```

## `trace`: run an agent

```bash
cargo run -p rusty-soar-cli -- trace <agent.soar> [--seed <n>] [--decisions <n>]
```

Loads the agent, runs it, and prints the decision trace and everything the agent writes, in the
same format as C++ Soar at its default trace level.

| Option | Default | Meaning |
|---|---|---|
| `<agent.soar>` | (required) | The file to load. Its `source` commands are followed. |
| `--seed <n>` | `0` | Seed for the random number generator, like C++ Soar's `srand <n>` |
| `--decisions <n>` | `10` | How many decision cycles to run. The run ends early if the agent halts. |

It is equivalent to this C++ Soar session:

```text
srand <n>
source <agent.soar>
run <n>
```

The command exits with status 1, printing the reason, if the file can't be read, contains an
unsupported command, or has a rule Soar would reject.

## Contract check (prototype)

```bash
cargo run -p rusty-soar-cli -- <rules.soar> <contract.aadl> [--target <triple>]
```

Reads a Soar file and an AADL file with an `annex agree` contract, runs the agent on two
built-in flight scenarios, checks the contract's guarantees on each, and then compiles the kernel
for the chosen target.

!!! warning "This is a scenario test, not a proof"

    The contract check only tries the two built-in scenarios. It is a prototype, and will be
    replaced by a verifier that proves an agent satisfies its AGREE contract for all inputs the
    contract allows.

| Option | Meaning |
|---|---|
| `--target <triple>`, `-t` | Rust target to compile for. Defaults to the computer you're on. |
| `--help`, `-h` | Lists the options and the supported targets. |
