#!/usr/bin/env python3
"""Arcade Lens plugin: ISBN recognition. Protocol: JSON lines on stdin/stdout."""
import json
import re
import sys

CANDIDATE = re.compile(r"(?:ISBN(?:-1[03])?:?\s*)?((?:97[89][-\s]?)?(?:\d[-\s]?){9}[\dXx])\b")


def valid(isbn: str) -> bool:
    if len(isbn) == 13 and isbn.isdigit():
        total = sum(int(d) * (1 if i % 2 == 0 else 3) for i, d in enumerate(isbn))
        return total % 10 == 0 and isbn[:3] in ("978", "979")
    if len(isbn) == 10 and isbn[:9].isdigit() and (isbn[9].isdigit() or isbn[9] in "Xx"):
        total = sum((10 - i) * (10 if c in "Xx" else int(c)) for i, c in enumerate(isbn))
        return total % 11 == 0
    return False


def recognize(params):
    text = params["input"].get("text", "")
    found = []
    for m in CANDIDATE.finditer(text):
        digits = re.sub(r"[-\s]", "", m.group(1)).upper()
        if valid(digits) and digits not in [f["text"] for f in found]:
            found.append({"capability": "dev.example.isbn", "text": digits, "confidence": 0.95, "details": [["ISBN", digits]]})
    return {"detections": found}


def execute(params):
    isbn = params["input"].get("text", "")
    if params["action"] == "dev.example.isbn.lookup":
        return {"message": f"Looking up {isbn}", "effects": [{"type": "open-uri", "uri": f"https://openlibrary.org/isbn/{isbn}"}]}
    if params["action"] == "dev.example.isbn.copy":
        return {"message": "Copied ISBN", "effects": [{"type": "copy-text", "text": isbn}]}
    raise ValueError("unknown action")


for line in sys.stdin:
    req = json.loads(line)
    try:
        handler = {"recognize": recognize, "execute": execute}[req["method"]]
        reply = {"id": req["id"], "result": handler(req["params"])}
    except Exception as e:  # report, never crash the protocol
        reply = {"id": req["id"], "error": str(e)}
    print(json.dumps(reply), flush=True)
