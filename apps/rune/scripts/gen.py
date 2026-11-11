"""Writes the headless input scripts snap.sh plays, into the folder given as the only argument.

Every script types the same three paragraphs, one key event per character, then does its
scene's step. Each script has an `end` mark at the moment its snapshot should catch.
"""

import json
import sys
from pathlib import Path

EVERY = 0.02
PARAGRAPHS = [
    "The kettle had been singing for a while before she noticed it.",
    "She had come down in the dark without turning on a single light, because the house felt "
    "different at that hour, as if it belonged to someone else and she was only borrowing it "
    "until morning, and every board on the stairs seemed to know exactly where she meant to put her feet.",
    "Outside, the first bus went past with nobody on it.",
]
SELECT_BACK = len("with nobody on it.")


def typed():
    events, t = [], 0.2
    for i, p in enumerate(PARAGRAPHS):
        if i:
            for _ in range(2):
                events.append({"t": round(t, 3), "type": "key", "name": "Enter"})
                t += 0.05
        events.append({"t": round(t, 3), "type": "type", "text": p, "every": EVERY})
        t += len(p) * EVERY + 0.1
    return events, t


def key(t, name, **mods):
    return {"t": round(t, 3), "type": "key", "name": name, **mods}


def mark(t):
    return {"t": round(t, 3), "type": "mark", "name": "end"}


def scenes():
    base, t = typed()
    yield "typed", base + [mark(t + 0.8)]
    yield "continue-streaming", base + [key(t + 0.8, "j", cmd=True), mark(t + 1.15)]
    yield "continue-accepted", base + [key(t + 0.8, "j", cmd=True), key(t + 2.0, "Tab"), mark(t + 2.4)]
    sel = [key(t + 0.3 + i * 0.02, "Left", shift=True) for i in range(SELECT_BACK)]
    u = t + 0.4 + SELECT_BACK * 0.02
    bar = [key(u, "k", cmd=True), {"t": round(u + 0.2, 3), "type": "type", "text": "make it lonelier", "every": EVERY}]
    v = u + 0.3 + 16 * EVERY
    yield "rewrite-proposal", base + sel + bar + [key(v, "Enter"), mark(v + 1.2)]
    yield "rewrite-bar", base + sel + bar + [mark(v)]
    yield "settings", base + [key(t + 0.5, ",", cmd=True), mark(t + 1.0)]
    yield "missing-key", base + [key(t + 0.5, "j", cmd=True), mark(t + 1.0)]
    yield "undo", base + [key(t + 0.3, "z", cmd=True), mark(t + 0.8)]


out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
for name, events in scenes():
    (out / f"{name}.json").write_text(json.dumps(events, indent=1))
