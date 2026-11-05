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
| `(abs a)` | Absolute value |
| `(int a)` | Converts to an integer, dropping any fraction. Also converts number-like strings. |
| `(float a)` | Converts to a float. Also converts number-like strings. |

Dividing by zero, or giving a non-number where a number is needed, makes the function fail; the
action that used it is skipped, as in C++ Soar.

## Symbols

| Function | Result |
|---|---|
| `(make-constant-symbol)` | `constant`, or `constant1`, `constant2`, ... if that name is taken |
| `(make-constant-symbol a b ...)` | A **new** symbol: the arguments joined, followed by a number, such as `foo1` |
| `(ifeq a b then else)` | `then` if `a` and `b` are the same symbol, otherwise `else` |
| `(crlf)` | A newline, for use with `write` |

## Actions

| Function | Effect |
|---|---|
| `(write a b ...)` | Writes the arguments to the agent's output, with no spaces between them |
| `(halt)` | Stops the agent after the current phase |

Numbers are written the way C++ Soar writes them: floats always with six decimal places, so
`(write (/ 7 2.0))` writes `3.500000`.
