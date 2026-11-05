# Writing Rules

Rusty-Soar reads the same `sp` syntax as C++ Soar, using a port of Soar's own lexer and parser.
Existing `.soar` files load as they are, as long as they only use supported features.

## Loading rules

From Rust, parse the text and install it on an agent:

```rust
let source = std::fs::read_to_string("agent.soar")?;
let script = SoarScript::parse(&source)?;
agent.install_soar_script(&script, state)?;
```

From the command line, `rusty-soar-cli trace` loads a file and follows its `source` commands:

```bash
cargo run -p rusty-soar-cli -- trace path/to/load.soar
```

## What a script can contain

| Contents | Supported |
|---|---|
| `sp {...}` rules | Yes, see below |
| `source file.soar` (also `"file"` and `{file}`) | Yes, from the command-line tool |
| Settings commands: `rl`, `decide`, `srand`, `watch`, `echo` | Yes, see [Script Commands](../reference/script-commands.md) |
| Simple Tcl rule generators (`proc` with `foreach`, see below) | Yes |
| Comments (`#`) and `;` between commands | Yes |
| Any other command | **No: loading fails with an error naming it** |

!!! danger "Why unsupported commands stop the load"

    If an agent's files turn on a feature Rusty-Soar doesn't have (`learn --on`, for example),
    running it anyway would give results that look like Soar's but aren't. So Rusty-Soar
    refuses, and tells you which command it couldn't run. Remove the command, or ask for the
    feature to be ported.

## Rule syntax

Everything in Soar's rule grammar is supported, except the items listed below. That includes:

- conditions on `state` and `impasse`, attribute paths (`^io.input-link.altitude`), negated
  conditions `-( ... )` and negated conjunctions `-{ ... }`
- tests: `<var>`, constants, `<>`, `<`, `<=`, `>`, `>=`, `<=>`, disjunctions `<< a b >>`, and
  conjunctions `{ ... }`
- all preferences, including binary ones (`>`, `<`, `=`) and numeric indifferent `= 0.5`
- documentation strings, and the flags `:o-support`, `:i-support`, `:default` and `:chunk`
- right-hand-side functions listed in [RHS Functions](../reference/rhs-functions.md)

Not yet supported:

- semantic-memory tests (`@`) and `$$` tests
- RHS functions not listed in [RHS Functions](../reference/rhs-functions.md)
- `gp` (rule generation) and RL `:template` rules
- the `:interrupt` flag

A rule with an unsupported flag is rejected when it is installed, with an error naming the rule.

Rules are checked the way Soar checks them: for example, a rule whose conditions don't all
connect to a state is rejected, as Soar rejects it.

## Tcl rule generators

Many agents use small Tcl procedures to write several similar rules at once. Rusty-Soar expands
this common form:

```tcl
proc generateWarn { types } {
   foreach type $types {
      sp "warn*${type}
            (state <s> ^${type}-error <e>)
         -->
            (<s> ^warning ${type})"
   }
}

generateWarn { gps lidar imu }
```

This produces three rules, `warn*gps`, `warn*lidar` and `warn*imu`. Other Tcl (variables,
`if`, `expr` and so on) is not supported.

!!! tip

    If your generators are more complicated, load the agent in C++ Soar once, print all of its
    productions in full, save that output as a `.soar` file, and load the saved file instead.
