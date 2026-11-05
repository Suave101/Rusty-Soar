# Comparing with C++ Soar

Rusty-Soar's main promise is that it behaves like C++ Soar. You can check that on your own
agents by running them in both and comparing what they print.

## How the comparison works

`differential/compare.sh` runs each agent twice, with the same random seed and number of
decisions:

- in C++ Soar, with its default trace level (`watch 1`);
- in Rusty-Soar, with `rusty-soar-cli trace`, which prints the same trace format.

It then compares the decision traces and everything the agents `write`, line by line.

## Step 1: build C++ Soar

In a checkout of [SoarGroup/Soar](https://github.com/SoarGroup/Soar):

```bash
python3 scons/scons.py kernel cli --no-svs
```

This puts the command-line program at `out/soar`.

## Step 2: run the comparison

From the Rusty-Soar repository:

```bash
differential/compare.sh /path/to/Soar/out/soar --seed 1 --decisions 20
```

With no agents named, it compares every agent in `differential/agents/`. To compare your own:

```bash
differential/compare.sh /path/to/Soar/out/soar my-agent/load.soar
```

## Reading the result

For each agent you get either:

```text
same: count
```

or a diff showing where the two runs differ, for example:

```diff
--- upstream/impasses
+++ rusty-soar/impasses
@@ -2,3 +2,3 @@
      1:    ==>S: S2 (operator tie)
-     2:    O: O2 (b)
+     2:    O: O1 (b)
```

The script exits with status 0 only if every agent matches.

!!! info "Expected differences"

    A few differences are known and documented in
    [Differences from C++ Soar](../reference/differences.md). If you find one that isn't listed
    there, please report it with the agent file that shows it.

## Included test agents

| Agent | What it exercises |
|---|---|
| `count` | operators, o-support, arithmetic, printing numbers |
| `indifferent` | random choice between equal operators, which tests the random number generator |
| `impasses` | tie, operator no-change and state no-change impasses |
| `conflict` | conflict and constraint-failure impasses |
