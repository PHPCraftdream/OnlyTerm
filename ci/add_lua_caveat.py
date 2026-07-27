#!/usr/bin/env python3
"""
Insert a standard "pending rhai conversion" caveat admonition immediately
before the first top-level ```lua fence in each given file.

Used for the docs/config/reference pages whose Lua examples involve real
control flow (loops, custom function definitions, if/then/else) that the
mechanical lua_to_rhai_docs.py converter deliberately does not attempt to
translate automatically, to avoid silently shipping incorrect rhai syntax
on pages users are likely to copy-paste from.
"""
import re
import sys

CAVEAT = """!!! warning "Pending rhai conversion"

    The code example(s) below still use Lua syntax from before OnlyTerm's
    config engine switched to rhai. The *option names, event names and
    object/method shapes* are unchanged -- only the scripting syntax differs.
    See the [migration guide](../migration-lua-to-rhai.md) for the Lua-to-rhai
    syntax mapping to translate this example yourself, or watch for a
    follow-up documentation pass that rewrites it directly.

"""


def compute_caveat(path: str) -> str:
    # relative path from this file up to docs/ so the migration guide link
    # in the caveat resolves correctly regardless of nesting depth.
    import os
    rel = os.path.relpath("docs", os.path.dirname(path)).replace("\\", "/")
    # rel is e.g. "..", "../.." etc. We want the number of "../" segments.
    depth = rel.count("..")
    up = "../" * depth
    return CAVEAT.replace("../migration-lua-to-rhai.md", f"{up}migration-lua-to-rhai.md")


FENCE_AT_BOL = re.compile(r"^```lua$", re.M)


def process(path):
    with open(path, encoding="utf-8") as f:
        content = f.read()
    if "Pending rhai conversion" in content:
        return False
    m = FENCE_AT_BOL.search(content)
    if not m:
        return False
    caveat = compute_caveat(path)
    new_content = content[:m.start()] + caveat + content[m.start():]
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    return True


def main():
    changed = 0
    for p in sys.argv[1:]:
        if process(p):
            changed += 1
    print(f"added caveat to {changed} files")


if __name__ == "__main__":
    main()
