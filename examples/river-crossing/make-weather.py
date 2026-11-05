"""Writes weather.rec, the river's clock and weather for each decision, as a recording Rusty-Soar replays.

Mostly calm, with spells of strong current and the odd storm. The seed is fixed so the file is
the same each time; change it, or the lengths below, for different weather.

    python3 make-weather.py > weather.rec
"""

import random

DECISIONS = 2500
LEARNING_OFF_AT = 2200

rng = random.Random(7)
weather = []
while len(weather) < DECISIONS:
    weather += [("calm", "no")] * rng.randint(20, 60)
    if rng.random() < 0.6:
        weather += [("strong", "no")] * rng.randint(3, 15)
    if rng.random() < 0.4:
        weather += [("calm", "yes")] * rng.randint(2, 8)
weather = weather[:DECISIONS]

print("# River weather, made by make-weather.py. After decision 1, the weather is listed only when it changes.")
last = (None, None)
for decision, (current, storm) in enumerate(weather, start=1):
    print(f"decision {decision}")
    if decision == LEARNING_OFF_AT:
        print("# From here on, the agent uses what it learned.")
        print("command rl --set learning off")
        print("command decide indifferent-selection --epsilon 0")
    print(f"river.time {decision}")
    if current != last[0]:
        print(f"river.current {current}")
    if storm != last[1]:
        print(f"river.storm {storm}")
    last = (current, storm)
