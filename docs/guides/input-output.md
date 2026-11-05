# Inputs and Outputs

This guide covers connecting an agent to the world: giving it sensor data and acting on its
commands. [Getting Started, step 4](../getting-started/inputs-outputs.md) shows a complete
example.

## The I/O structure

Every agent's top state has this structure, created by `create_top_state()`:

```text
(S1 ^io I1)
(I1 ^input-link I2)
(I1 ^output-link I3)
```

| Method | Returns |
|---|---|
| `agent.input_link()` | `I2`, where your program writes |
| `agent.output_link()` | `I3`, where the agent writes |

## Giving the agent input

Add facts to the input link with `add_input_wme`, and remove them with `remove_input_wme`:

```rust
let speed = agent.symbols.intern_str("speed");
let value = agent.symbols.intern_int(250);
let key = agent.add_input_wme(input_link, speed, value);   // (I2 ^speed 250)

// later...
agent.remove_input_wme(key);
```

Call these **between** decision cycles. This is equivalent to C++ Soar's input callback, which
runs during the input phase: nothing happens between one cycle's output phase and the next
cycle's input phase.

To build nested structure, make a new identifier and link to it:

```rust
let gps = agent.new_input_identifier('G');                 // G1
let gps_attr = agent.symbols.intern_str("gps");
agent.add_input_wme(input_link, gps_attr, gps);            // (I2 ^gps G1)
```

Things to know:

- **To change a value, remove the old WME and add a new one.** Soar treats the new WME as a new
  fact, so rules that matched the old value retract and match again.
- **Input WMEs stay until you remove them.** Rules can't remove them.
- **Adding the same fact twice makes two WMEs**, as in C++ Soar.
- `remove_input_wme` returns `false`, and writes Soar's error message to the agent's output,
  if the WME isn't a current input WME.

## Reading the agent's output

After each cycle, `take_output_events()` returns what changed on the output link:

```rust
for event in agent.take_output_events() {
    // event.mode is Added, Modified or Removed
    for wme in &event.wmes {
        // every WME reachable from the output link
    }
}
```

| `event.mode` | When |
|---|---|
| `Added` | The output link was just created (not reported when the agent is created) |
| `Modified` | Something reachable from the output link changed this cycle |
| `Removed` | The output link itself was removed |

Each event lists **all** the WMEs reachable from the output link, not only the changed ones,
exactly as C++ Soar passes them to output functions. If nothing changed, there is no event.

`agent.output_link_changed()` tells you whether the agent *added* to its output link since the
last input phase, which is useful for "run until output" loops.

!!! note "Command status"

    C++ Soar's client libraries (SML) mark commands with `^status complete` once handled. That is
    a convention of those libraries, not part of the kernel, so Rusty-Soar leaves it to your
    program: add `(command ^status complete)` as input if your rules expect it.

## Replaying recorded input

An agent that reads from a simulator can be rerun without it: record what the simulator put on
the input link each decision, then replay it.

```bash
cargo run -p rusty-soar-cli -- trace agent.soar --input run.rec
```

A recording is a text file with one block per decision:

```text
# comments start with #
decision 1
flightdata.airspeed 0
flightdata.lat 28.0628
flightdata.failures.engine-0 |ok|
command rl --set learning on
decision 2
flightdata.airspeed 3
```

- **A path** names attributes from the input link down. Identifiers along it are made the first
  time it's used, and each path holds one value.
- **Values** are written as in a rule: `3` is an integer, `3.0` a float, `|ok|` or `ok` a string.
  Integers and floats are different values, as in Soar.
- **`command`** runs a script command before the decision, for settings the environment changes
  between decisions.
- **A value that hasn't changed changes nothing.** A different value replaces the WME, so rules
  that matched the old one retract and match again. This is what JSoar's `InputWme.update` does,
  so a recording can list every value each decision, or only the ones that changed.

The lines of a block run in order, before that decision. The run has one decision per block
unless `--decisions` says otherwise; after the last block, the input stays as it is.

In Rust, `InputRecording::parse` and `InputReplay::apply_next` do the same for your own loop.

### Example: missionaries and cannibals in bad weather

`examples/river-crossing` is the classic puzzle with a twist: the river reports its weather on
the input link. A strong current means the boat holds one person, not two, and nobody crosses in
a storm. `weather.rec` is 2500 decisions of recorded weather, made by `make-weather.py`, and
includes a clock that ticks each decision.

```bash
cargo run -p rusty-soar-cli -- trace examples/river-crossing/load.soar --seed 1 --input examples/river-crossing/weather.rec
```

The agent solves the puzzle, starts over and learns with RL. A macro (`proc`/`foreach`)
generates its 192 RL rules, one for each move from each bank and boat side. Its first trip takes
51 crossings. At decision 2200 the recording sends `command rl --set learning off` and
`command decide indifferent-selection --epsilon 0`, and every trip after that takes the minimum,
11. Under a strong current, it learned to wait for calm water.

Because the weather doesn't depend on what the agent does, a replay is exactly what happened.
With an environment that reacts to the agent's moves, a replay is exact only while the agent
makes the same choices it made when the run was recorded.

### Recording from JSoar

`differential/recorders/jsoar/InputRecorder.java` writes this format from a JSoar environment.
Copy it into your project's `recording` package, then:

```java
InputRecorder recorder = new InputRecorder("run.rec");

// beside each command the environment gives between decisions
agent.getInterpreter().eval("rl --set learning on");
recorder.command("rl --set learning on");

// after updating the input, just before each decision
recorder.recordDecision(agent);
agent.runFor(1, RunType.DECISIONS);
```

It copies the whole input link each decision, oldest WME first.
