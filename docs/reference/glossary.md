# Glossary

Acceptable preference
:   A `+` preference: it makes an operator a candidate.

Apply phase
:   The phase where rules carry out the selected operator.

Attribute
:   The middle part of a WME, written with `^`, as in `^altitude`.

Decision cycle
:   One pass through the input, propose, decide, apply and output phases.

Elaboration
:   A rule firing that works out new facts rather than proposing or applying an operator.

Identifier
:   A node in working memory, such as `S1` or `I2`: a letter and a number.

I-support
:   Instantiation support. A WME with i-support is removed as soon as the rule that made it stops
    matching.

Impasse
:   A situation where Soar cannot make a decision. Soar responds by creating a substate.

Input link
:   `I2`, the identifier where your program puts sensor data.

Instantiation
:   One match of a rule's conditions: the rule plus the particular WMEs it matched.

O-support
:   Operator support. A WME made while applying an operator stays until something removes it.

Operator
:   An action the agent can take. Rules propose operators, Soar selects one, and rules apply it.

Output link
:   `I3`, the identifier where the agent puts commands for your program.

Preference
:   A statement about an operator: acceptable, better, worse, best, worst, indifferent, numeric,
    require or reject.

Production
:   Another word for rule.

Rete
:   The algorithm Soar uses to match all rules against working memory efficiently.

Reward link
:   `^reward-link` on every state. Rules put `(^reward.value <number>)` there to give
    reinforcement-learning reward.

RL rule
:   A rule whose only action is a numeric preference. Reinforcement learning adjusts its value.

Rule
:   Knowledge in the form "if these conditions match, do these actions", written with `sp`.

State
:   The top of a working-memory graph. `S1` is the top state; substates are created by impasses.

Substate
:   A state Soar creates below another when it reaches an impasse, so rules can work out what to
    do.

Symbol
:   A value in working memory: a string, integer, float or identifier.

Top state
:   `S1`, the state every agent starts with.

Value
:   The last part of a WME: a number, a string, or an identifier.

WME
:   Working memory element: one fact, written `(identifier ^attribute value)`.

Working memory
:   Everything the agent currently knows, as a graph of WMEs.
