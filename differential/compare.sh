#!/usr/bin/env bash
# Differential test against upstream Soar: runs each agent under upstream and under Rusty-Soar
# with the same random seed and decision count, and diffs the default (`watch 1`) decision
# trace together with `write` output.
#
# Usage: compare.sh <upstream-soar-binary> [--seed N] [--decisions N] [agent.soar ...]
#
# With no agents given, every agent in agents/ is compared. Build upstream yourself first, for
# example in a checkout of github.com/SoarGroup/Soar:
#   python3 scons/scons.py kernel cli --no-svs
# which puts the command-line binary at out/soar.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
manifest="$here/../rusty-soar/Cargo.toml"

if [[ $# -lt 1 ]]; then
    sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
fi
upstream="$(realpath "$1")"
shift

seed=1
decisions=20
agents=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --seed) seed="$2"; shift 2 ;;
        --decisions) decisions="$2"; shift 2 ;;
        *) agents+=("$(realpath "$1")"); shift ;;
    esac
done
if [[ ${#agents[@]} -eq 0 ]]; then
    agents=("$here"/agents/*.soar)
fi

cargo build --release -q -p rusty-soar-cli --manifest-path "$manifest"
rusty="$(dirname "$manifest")/target/release/rusty-soar-cli"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Keeps the trace and write output: drops colour codes, trailing blanks, empty lines, the
# initial `0: ==>S: S1` line (upstream prints it when the agent is created, possibly before
# its output reaches the CLI) with any `*` (one per rule) and `@` (one per `smem --add`
# memory) markers loading prints, and upstream's sourcing and halt messages.
normalize() {
    sed -e 's/\x1b\[[0-9;]*m//g' -e 's/[[:space:]]*$//' |
        grep -vE '^$|^[[:space:]]*0: ==>S: S1( *[*@]+)?$|^Sourcing |^[*@]+$|^Total: |^Source finished|^This Agent halted\.$|^An agent halted|^Interrupt received\.$' ||
        true
}

failed=0
for agent in "${agents[@]}"; do
    name="$(basename "$agent" .soar)"
    driver="$work/$name.driver.soar"
    printf 'srand %s\nsource "%s"\n' "$seed" "$agent" > "$driver"

    "$upstream" -n -s "$driver" run "$decisions" 2>&1 | normalize > "$work/$name.upstream"
    "$rusty" trace "$agent" --seed "$seed" --decisions "$decisions" 2>&1 | normalize > "$work/$name.rusty"

    if diff -u --label "upstream/$name" --label "rusty-soar/$name" "$work/$name.upstream" "$work/$name.rusty"; then
        echo "same: $name"
    else
        failed=1
    fi
done
exit "$failed"
