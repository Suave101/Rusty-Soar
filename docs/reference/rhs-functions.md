# RHS Functions

Functions that rule actions (the right-hand side) can call. They behave as in C++ Soar. Using a
function not listed here is an error when the rule is loaded.

## Arithmetic

| Function | Result |
|---|---|
| `(+ a b ...)` | Sum. An integer, unless any argument is a float. |
| `(* a b ...)` | Product. An integer, unless any argument is a float. |
| `(- a)` | `a` negated |
| `(- a b ...)` | `a` minus the rest |
| `(/ a b ...)` | `a` divided by the rest. **Always a float.** `(/ a)` is `1 / a`. |
| `(div a b)` | Integer division (integers only) |
| `(mod a b)` | Integer remainder (integers only) |
| `(min a b ...)`, `(max a b ...)` | The smallest or largest. An integer, unless any argument is a float. |
| `(abs a)` | Absolute value |
| `(sqrt a)`, `(sin a)`, `(cos a)` | Square root, sine and cosine (radians), as floats |
| `(atan2 y x)` | The angle of `(x, y)` in radians, as a float |
| `(int a)` | Converts to an integer, dropping any fraction. A string converts as C's `strtoll` does: its leading digits, or 0 if there are none. |
| `(float a)` | Converts to a float. A string converts as C's `strtod` does. |
| `(rand-int)`, `(rand-int n)` | A random integer from 0 to 2³² − 1, or from 0 to `n` |
| `(rand-float)`, `(rand-float n)` | A random float from 0 to 1, or from 0 to `n` |

Random numbers come from the same generator as operator selection, so `srand` fixes them too.

Dividing by zero, or giving a non-number where a number is needed, writes C++ Soar's error
message and makes the function fail; the action that used it is skipped, as in C++ Soar.

## Sets

These take an identifier and an attribute, and work on the values of that attribute. One or two
more attributes go deeper: `(set-sum <s> item value)` sums the `^value`s of each `^item`.

| Function | Result |
|---|---|
| `(set-count <s> attr ...)` | How many values there are |
| `(set-sum <s> attr ...)`, `(set-multiply ...)` | Sum or product of the numbers, as a float |
| `(set-min ...)`, `(set-max ...)`, `(set-range ...)` | Smallest, largest, and largest minus smallest, as floats |
| `(set-mean ...)`, `(set-stdev ...)` | Mean and (population) standard deviation, as floats |
| `(size <s>)` | How many WMEs the identifier has |
| `(sum <s>)`, `(product <s>)` | Sum or product of all the identifier's values, read as integers |

Values that aren't numbers are left out; with no numbers, the result is the string `NaN`. As in
C++ Soar, these functions see WMEs made by rules, not input WMEs.

## Headings and positions

These come from C++ Soar's air-combat and Quake agents. Headings are compass degrees: 0 is
along +y and 90 along +x.

| Function | Result |
|---|---|
| `(compute-heading x1 y1 x2 y2)` | The heading from one point to the other, in whole degrees from −179 to 180 |
| `(compute-range x1 y1 x2 y2)` | The distance between them, as an integer |
| `(round-off n m)` | `n` rounded to the nearest multiple of `m` (to tenths if `m` is a float) |
| `(round-off-heading n m)` | The same, then brought into −180 to 180 |
| `(extrapolate-x-position x heading speed time)`, `(extrapolate-y-position y ...)` | Where a mover will be, with +y to the south |
| `(haversine lat1 lon1 lat2 lon2)` | Great-circle distance in kilometres |
| `(select-point-closest-to-vector x y heading <points>)` | The `^id` of the point (`^x`, `^y`, `^id`) whose bearing is closest to the heading |
| `(compute-dice-probability dice sides count pred)` | The chance that `count` dice show a given face, for `pred` `eq`, `ne`, `lt`, `gt`, `le` or `ge` |

## Symbols and strings

| Function | Result |
|---|---|
| `(make-constant-symbol)` | `constant`, or `constant1`, `constant2`, ... if that name is taken |
| `(make-constant-symbol a b ...)` | A **new** symbol: the arguments joined, followed by a number, such as `foo1` |
| `(concat a b ...)` | The arguments joined into one string |
| `(string a)` | The argument as a string; floats keep all their digits |
| `(strlen a)` | The length of the argument as `write` writes it |
| `(capitalize-symbol a)` | The string with its first letter capitalized |
| `(trim a)` | The string without leading and trailing spaces, tabs and newlines |
| `(ifeq a b then else)` | `then` if `a` and `b` are the same symbol, otherwise `else` |
| `(crlf)` | A newline, for use with `write` |
| `(dc)` | How many decision phases have run |
| `(@ <x>)` | The number of the long-term identifier `<x>` is linked to in semantic memory; fails if it isn't linked |

## Actions

| Function | Effect |
|---|---|
| `(write a b ...)` | Writes the arguments to the agent's output, with no spaces between them |
| `(trace n a b ...)` | Writes the rest if trace setting `n` (0 to 5) is on: 1 is on at `trace 1` and above, 2 at `trace 2`, and 3 to 5 at `trace 3` |
| `(log n a b ...)` | Writes the rest to output channel `n` (1 to 100); all channels are on |
| `(halt)` | Stops the agent after the current phase |
| `(count a b ...)` | Adds one to a counter named by each argument (`SoarAgent::dyn_counters`) |
| `(wait ms)` | Accepted, but doesn't wait: C++ Soar sleeps, which changes nothing in a run |
| `(link-stm-to-ltm <x> n)` | Links identifier `<x>` to long-term identifier `@n` |
| `(force-learn <s>)` | Flags a state for learning under `chunk only` |
| `(dont-learn <s>)` | Flags a state as not learning under `chunk except` |

Numbers are written the way C++ Soar writes them: floats always with six decimal places, so
`(write (/ 7 2.0))` writes `3.500000`.

## Not supported

`accept` (reads a line from the keyboard), `timestamp` (reads the clock), `deep-copy`,
`from-st-xml`, and SML's `exec`, `cmd` and `interrupt`.
