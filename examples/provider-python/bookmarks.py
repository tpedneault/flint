#!/usr/bin/env python3
"""A Flint provider in Python, standard library only: your bookmarks.

Reads `bookmarks.txt` beside this script, one bookmark per line as
`Title | https://url` (a built-in sample list when the file is missing),
and gives them to Flint as static items, which Flint itself matches and
ranks. Each opens or copies its URL, actions Flint carries out without
asking back, and offers "Count bookmarks", a `provider` action Flint
sends here as `execute`.

Declare it in Flint's settings.toml:

    [[providers]]
    name = "bookmarks"
    command = ["python", "path/to/bookmarks.py"]

See docs/PROVIDERS.md for the protocol; this file is meant to be copied.
"""

import json
import os
import sys

NAME = "bookmarks"
SAMPLE = [
    ("Rust standard library", "https://doc.rust-lang.org/std/"),
    ("Fenix on GitHub", "https://github.com/tpedneault/fenix"),
    ("Flint on GitHub", "https://github.com/tpedneault/flint"),
]


def read_message(stream):
    """One `Content-Length`-framed JSON message, or None at end of input."""
    length = None
    while True:
        line = stream.readline()
        if not line:
            return None
        line = line.decode("ascii").strip()
        if not line:
            break
        key, _, value = line.partition(":")
        if key.strip().lower() == "content-length":
            length = int(value.strip())
    return json.loads(stream.read(length).decode("utf-8"))


def write_message(stream, message):
    body = json.dumps(message).encode("utf-8")
    stream.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    stream.flush()


def bookmarks():
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bookmarks.txt")
    try:
        with open(path, encoding="utf-8") as f:
            lines = [line.split("|", 1) for line in f if "|" in line]
        return [(title.strip(), url.strip()) for title, url in lines]
    except OSError:
        return SAMPLE


def item(title, url):
    return {
        "id": f"{NAME}:{url}",  # stable across runs: frecency is keyed on it
        "title": title,
        "subtitle": url,
        "icon": "link",
        "actions": [
            {"id": "open", "title": "Open", "key": "o", "kind": "open", "target": url},
            {"id": "copy", "title": "Copy URL", "key": "c", "kind": "copy", "text": url},
            {"id": "count", "title": "Count bookmarks", "key": "n", "kind": "provider"},
        ],
    }


def main():
    stdin, stdout = sys.stdin.buffer, sys.stdout.buffer
    marks = bookmarks()
    write_message(stdout, {"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
        "protocol": 1, "role": "provider", "name": NAME, "version": "1.0", "pid": os.getpid()}})
    read_message(stdin)  # the host's answer; it closes the pipe if it refuses
    write_message(stdout, {"jsonrpc": "2.0", "id": 2, "method": "provider/register", "params": {
        "id": NAME, "title": "Bookmarks", "prefix": "b",
        "items": [item(title, url) for title, url in marks]}})
    write_message(stdout, {"jsonrpc": "2.0", "method": "log", "params": {
        "level": "info", "message": f"serving {len(marks)} bookmarks"}})

    while (message := read_message(stdin)) is not None:
        if message.get("method") != "execute":
            continue  # register's answer, queries (we're static), cancels
        params = message.get("params", {})
        if params.get("actionId") == "count":
            result = {"close": False, "message": f"{len(marks)} bookmarks"}
            write_message(stdout, {"jsonrpc": "2.0", "id": message["id"], "result": result})
        else:
            error = {"code": -32010, "message": f"no action {params.get('actionId')}"}
            write_message(stdout, {"jsonrpc": "2.0", "id": message["id"], "error": error})


if __name__ == "__main__":
    main()
