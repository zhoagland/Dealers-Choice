"""An example external bot for Dealers Choice (PRD FR-BT-8, docs/tech/bots.md §6).

Reads one JSON decision request per line on stdin and writes one reply per line on stdout:

    request: {"model", "game_id", "game_version", "observation", "legal", "legal_labels"}
    reply:   {"probs": [p0, p1, ...]}   one probability per legal action (any non-negative scale)

This one plays a simple style from the action labels alone, so it works for every game: it
prefers checking and calling, raises a little, and folds only when it must. Replace `decide`
with your own model. Run it through the engine with:

    dc sim --game holdem_nl --bots ext:config/external/example.toml,ismcts:3 --deals 200
"""

import json
import sys


def decide(request):
    probs = []
    for label in request["legal_labels"]:
        text = label.lower()
        if text.startswith("check") or text.startswith("call"):
            probs.append(6.0)
        elif text.startswith("fold"):
            probs.append(0.5)
        elif text.startswith(("bet", "raise", "r ")):
            probs.append(1.0)
        else:
            probs.append(2.0)  # draws, declarations, card choices: spread evenly
    return probs


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    reply = {"probs": decide(json.loads(line))}
    sys.stdout.write(json.dumps(reply) + "\n")
    sys.stdout.flush()
