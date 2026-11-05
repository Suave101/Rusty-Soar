# Differences from C++ Soar

Rusty-Soar aims to behave exactly like C++ Soar. Where it can't, or where C++ Soar's own
behavior depends on something outside the algorithm, the difference is listed here.

## Deliberate differences

| Area | C++ Soar | Rusty-Soar | Why |
|---|---|---|---|
| Default random seed | Seeded from the clock | Seeded with 0 | Runs must be reproducible |
| `srand` with no seed | Seeds from the clock | Error | Same reason |
| Unsupported commands | Run (they exist in C++ Soar) | Loading fails with an error | An agent must never quietly run differently from its files |
| Memory | Grows as needed | Fixed sizes chosen when the agent is created; the agent halts with an error if one fills | Bounded memory is needed for verification and certification |

## Order of things that are sets

C++ Soar sometimes lists things in an order that comes from how its memory is laid out. These
orders do not affect which decisions are made, but they can show up in output.

| Where | Effect |
|---|---|
| Trace lines for objects with several `^name` values | The values may be listed in a different order |
| WMEs within one identifier in an output event | May be in a different order (all the WMEs are there) |
| RL eligibility traces | C++ Soar orders them by memory address; Rusty-Soar by rule creation order. With several traced rules, floating-point sums can differ in the last digits. |
| Symbols | C++ Soar frees unused symbols; Rusty-Soar keeps them. This can change which number `make-constant-symbol` adds in rare cases. |

## Floating point

Rusty-Soar uses the `libm` library for `exp`, `log` and `pow`, where C++ Soar uses the C library
on your system. Results can differ in the last binary digit in rare cases.

## C++ Soar behavior copied on purpose

These may look like bugs, but Rusty-Soar copies them so results match:

- **RL value update.** After a reinforcement-learning update, the current numeric preferences of
  *every* rule that was updated are set to the new value of the *last* rule updated. C++ Soar
  does this through a variable left over from a loop.

## Not yet the same

| Area | Status |
|---|---|
| Results returned from substates | Kept with simplified bookkeeping instead of chunking's justifications; i-supported results can retract at different times |
| Order of rule firings within one elaboration wave | May differ; the set of firings is the same |
| Condition ordering in the Rete | Soar reorders conditions to match faster; Rusty-Soar keeps them in the order given. This affects speed, not results. |
| Trace levels above 1 | Only the level-1 decision trace is printed |
