# Differences from C++ Soar

Rusty-Soar aims to behave exactly like C++ Soar. Where it can't, or where C++ Soar's own
behavior depends on something outside the algorithm, the difference is listed here.

## Deliberate differences

| Area | C++ Soar | Rusty-Soar | Why |
|---|---|---|---|
| Default random seed | Seeded from the clock | Seeded with 0 | Runs must be reproducible |
| `srand` with no seed | Seeds from the clock | Error | Same reason |
| `learn` | Not a command in Soar 9.6 | Same as `chunk` | Lab agents written for JSoar 4 use it |
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
| Chunking | Learned rules' variable names and condition order can differ from C++ Soar's, and duplicates are found by comparing the rules as built, so rules that differ only in order aren't noticed. Identical conjunctive negations from two firings aren't merged. Justifications aren't added to the Rete: a result without a rule stays supported by the WMEs it depends on, and the justification is only used to trace through. With `automatically-create-singletons` on, every string attribute counts as a singleton, where upstream counts those seen in a WME while the setting is on. `chunk singleton` messages are shorter than upstream's. None of this has been compared with C++ Soar yet. |
| Episodic memory order | C++ Soar orders some of its sets by where objects sit in memory, which can change which of several equally good graph-match mappings a query reports. Rusty-Soar uses the order they're made in. The episode retrieved is not affected unless two mappings differ in whether they succeed. This has not yet been compared with C++ Soar. |
| Semantic memory order | C++ Soar keeps semantic memory in SQLite, and the order of retrieved WMEs, and which of two equally active memories a query returns, come from SQLite's indexes. Rusty-Soar reproduces the order of the index C++ Soar's queries use, but this has not yet been compared with C++ Soar. `smem --add` numbers new memories, and orders each memory's attributes, by where C++ Soar's objects happen to sit in memory; Rusty-Soar uses the order they're made in, which is usually the same. |
| Working-memory activation | C++ Soar frees a WME, and its activation, when nothing refers to it any more; Rusty-Soar checks for that once a decision, before updating the reference histories. A WME that has left working memory but is still referred to is never forgotten here (C++ Soar would remove the preferences of a WME with the same value). C++ Soar orders some of its sets by memory address, which can change the order forgotten WMEs are removed in. Matches upstream's `WmaFunctionalTests` and the episodic-memory tests that use activation; not yet compared with C++ Soar otherwise. |
| RL templates | C++ Soar finds a template's duplicate rule in the Rete; Rusty-Soar compares it with the rules templates have made before, so a rule that differs only in condition order isn't noticed. Off-policy GQ(λ) keeps each preference's `rho` from one decision to the next in C++ Soar; Rusty-Soar works it out each decision, which differs only when a decision has one candidate after one with several. Not yet compared with C++ Soar. |
