"""Unwrap hard-wrapped comments.

Usage: comment_wrap.py AST_GREP AST_GREP_CONFIG FILE...
"""

import json
import re
import subprocess
import sys
from collections import defaultdict

# indent, comment token, spaces after the token, text
LINE = re.compile(r"^(\s*)(///|//!|//|#|--)(?=\s|$)(\s*)(.*?)\s*$")
# markdown list item or haddock `-- |` prefix
MARKER = re.compile(r"^(?:[-*+|]|\d+[.)])\s+")
FENCE = re.compile(r"^(```|~~~)")
# endings that close a clause or open a code block or table row
ENDS_BLOCK = re.compile(r"[.,:!?;{\[=|\\]$")
# a lowercase start means the previous line broke mid-sentence
CONTINUES = re.compile(r"^[a-z(`'\"]")


def parse(line):
    m = LINE.match(line)
    if m is None:
        return None
    indent, token, gap, body = m.groups()
    return indent, token, len(gap), body


def comment_lines(ast_grep, config, files):
    """Map each file to the (line, column) of every line inside a comment node.

    Uses ast-grep to skip `#` and `//` inside strings.
    """
    out = subprocess.run(
        [
            ast_grep,
            "scan",
            "--config",
            config,
            # tree-sitter-toml is loaded as a native library
            "--custom-languages",
            "allow",
            "--json=stream",
            *files,
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    found = defaultdict(set)
    for raw in out.splitlines():
        node = json.loads(raw)
        start, end = node["range"]["start"], node["range"]["end"]
        # rust doc comments include their newline and end at column 0 of the next line
        last = end["line"] - (end["column"] == 0 and end["line"] > start["line"])
        # haskell merges consecutive `--` lines into one node
        found[node["file"]].update(
            (line, start["column"]) for line in range(start["line"], last + 1)
        )
    return found


def joinable(a, b):
    """Whether parsed comment line b continues the sentence on line a."""
    # same indent and comment token
    if a[:2] != b[:2]:
        return False
    _, _, a_gap, a_body = a
    _, _, b_gap, b_body = b
    # blank comment lines and markdown headings
    if not a_body or not b_body or a_body.startswith("#"):
        return False
    if FENCE.match(a_body) or FENCE.match(b_body):
        return False
    if ENDS_BLOCK.search(a_body) or not CONTINUES.match(b_body):
        return False
    marker = MARKER.match(a_body)
    # list items continue at the item text's column
    if marker:
        return b_gap == a_gap + marker.end()
    # prose has one space after the token, anything deeper is an indented example
    return a_gap == b_gap == 1


def rewrite(lines, eligible):
    """Join continuation lines top-down."""
    fenced = False
    i = 0
    while i < len(lines) - 1:
        a = parse(lines[i]) if eligible[i] else None
        b = parse(lines[i + 1]) if eligible[i + 1] else None
        if not fenced and a is not None and b is not None and joinable(a, b):
            cr = "\r" if lines[i].endswith("\r") else ""
            lines[i] = lines[i].rstrip() + " " + b[3] + cr
            del lines[i + 1]
            del eligible[i + 1]
            # retry the merged line against its new neighbor
            continue
        # a fence only spans one comment block
        if a is None:
            fenced = False
        elif FENCE.match(a[3]):
            fenced = not fenced
        i += 1
    return lines


def main():
    ast_grep, config, *files = sys.argv[1:]
    if not files:
        return
    for path, positions in comment_lines(ast_grep, config, files).items():
        with open(path, encoding="utf-8", newline="") as f:
            text = f.read()
        lines = text.split("\n")
        eligible = [False] * len(lines)
        for line, column in positions:
            parsed = parse(lines[line])
            # own-line comments only
            if parsed is not None and len(parsed[0]) == column:
                eligible[line] = True
        new = "\n".join(rewrite(lines, eligible))
        if new != text:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(new)


if __name__ == "__main__":
    main()
