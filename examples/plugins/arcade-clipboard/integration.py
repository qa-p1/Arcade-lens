#!/usr/bin/env python3
"""Generic Arcade integration bridge: forwards the selected text to an Arcade
app's command-line interface (argv[1:] + the text). Protocol: JSON lines."""
import json
import shutil
import subprocess
import sys

command = sys.argv[1:]

for line in sys.stdin:
    req = json.loads(line)
    try:
        if req["method"] != "execute":
            raise ValueError("this plugin only provides actions")
        text = req["params"]["input"].get("text")
        if text is None:
            raise ValueError("nothing to send")
        if shutil.which(command[0]) is None:
            raise RuntimeError(f"{command[0]} is not installed")
        done = subprocess.run(command + [text], capture_output=True, text=True, timeout=20)
        if done.returncode != 0:
            raise RuntimeError(done.stderr.strip() or f"{command[0]} failed")
        reply = {"id": req["id"], "result": {"message": done.stdout.strip() or "Done"}}
    except Exception as e:
        reply = {"id": req["id"], "error": str(e)}
    print(json.dumps(reply), flush=True)
