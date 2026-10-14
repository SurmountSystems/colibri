"""`just check` must name every Linux CI lint and test.

The gate is the text of `just --show check` plus the bodies of the recipes
it depends on. A make target that lives inside one of those recipes counts.
This does not run `just check`, and it does not claim Windows, macOS, musl,
or ARM ran.
"""

import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
JUSTFILE = ROOT / "justfile"
MAKEFILE = ROOT / "c" / "Makefile"

# Strings the coverage text must contain. `python` and `web` are the language
# names; the recipe names are python-test and web-check.
REQUIRED = [
    "rust-fmt",
    "rust-clippy",
    "rust-nextest",
    "c-check",
    "python",
    "web",
    "gguf",
    "sanitizers",
    "k3-tiny",
    "mimo-tiny",
    "v4-tiny",
    "glm53-oracle",
    "docker",
    "vulkan",
    "vulkan-engines",
    "engine-cuda-syntax",
    "engine-hip-syntax",
    "qwen36-tiny-check",
    "olmoe-tiny-check",
    "dsv41-tiny-check",
    "qwen38-tiny-check",
    "laya-tiny-check",
    "gliner-decide-tiny-check",
    "clef-tiny-check",
    "jev-sdk-check",
    "qwenimage-tiny-check",
    "inkling-oracle",
    "efficiency",
    "cluster-parity",
    "engine",
    "engines-linux",
]

# ci.yml job python builds these before discover.
ENGINE_LIST = (
    "kimi_k3 inkling colibri olmoe qwen38 qwen36 deepseek_v41 qwenimage"
)

# The prompt's include list. `qwen38-tiny-fp8-check` does not contain the
# contiguous substring tiny-check (the text is tiny-fp8-check). A target
# counts when the name contains both words, which is how those names and
# mimo-tiny-serve-check are included.
KNOWN_TINY_CHECKS = {
    "kimi-k3-tiny-check",
    "deepseek-v4-tiny-check",
    "mimo-tiny-check",
    "qwen38-tiny-check",
    "qwen38-tiny-fp8-check",
    "qwen38-tiny-int4-check",
    "qwen38-tiny-mtp-check",
    "laya-tiny-check",
    "gliner-decide-tiny-check",
    "clef-tiny-check",
}


def _recipe_header(line):
    """Return the recipe name if `line` starts a just recipe, else None."""
    match = re.match(r"^([A-Za-z_][A-Za-z0-9_-]*)(?:[ \t]+[^:\n]*)?:(?!=)", line)
    if not match:
        return None
    return match.group(1)


def recipe_texts(just_text):
    """Map recipe name to header plus indented body."""
    lines = just_text.splitlines()
    found = {}
    index = 0
    while index < len(lines):
        name = _recipe_header(lines[index])
        if name is None:
            index += 1
            continue
        header = [lines[index]]
        while header[-1].rstrip().endswith("\\") and index + 1 < len(lines):
            index += 1
            header.append(lines[index])
        index += 1
        body = []
        while index < len(lines):
            line = lines[index]
            if line.strip() == "" or line.startswith((" ", "\t")):
                body.append(line)
                index += 1
                continue
            break
        found[name] = "\n".join(header + body)
    return found


def _dependency_tokens(header_text):
    """Recipe names from a `name: dep dep (other \"1\")` header."""
    _name, sep, rest = header_text.partition(":")
    if not sep:
        return []
    names = []
    index = 0
    text = rest.strip()
    while index < len(text):
        while index < len(text) and text[index].isspace():
            index += 1
        if index >= len(text):
            break
        if text[index] == "(":
            depth = 0
            start = index
            while index < len(text):
                if text[index] == "(":
                    depth += 1
                    index += 1
                elif text[index] == ")":
                    depth -= 1
                    index += 1
                    if depth == 0:
                        break
                else:
                    index += 1
            inner = text[start + 1 : index - 1].strip()
            if inner:
                names.append(inner.split()[0].strip("\"'"))
            continue
        start = index
        while index < len(text) and not text[index].isspace():
            index += 1
        names.append(text[start:index])
    return names


def check_dependencies(show_text, just_text):
    """Direct dependency names of `check`, from `just --show` or the justfile."""
    for source in (show_text, just_text):
        lines = source.splitlines()
        index = 0
        while index < len(lines):
            if _recipe_header(lines[index]) != "check":
                index += 1
                continue
            header = [lines[index]]
            while header[-1].rstrip().endswith("\\") and index + 1 < len(lines):
                index += 1
                header.append(lines[index])
            flat = " ".join(part.replace("\\", " ") for part in header)
            return _dependency_tokens(flat)
    return []


def tiny_check_targets(makefile_text):
    """Target names on rule lines and .PHONY lines that contain tiny and check.

    Continuation backslashes are joined first. Recipe command lines (tab) and
    comments are not rules.
    """
    logical = []
    pending = None
    for line in makefile_text.splitlines():
        if pending is None:
            pending = line
        else:
            pending = pending[:-1].rstrip() + " " + line.strip()
        if pending.rstrip().endswith("\\"):
            continue
        logical.append(pending)
        pending = None
    if pending is not None:
        logical.append(pending)

    found = set()
    for line in logical:
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or line.startswith("\t"):
            continue
        if stripped.startswith(".PHONY:"):
            tokens = stripped.split(":", 1)[1].split()
        else:
            colon = stripped.find(":")
            if colon < 0:
                continue
            if colon + 1 < len(stripped) and stripped[colon + 1] == "=":
                continue
            tokens = stripped[:colon].split()
        for token in tokens:
            if "tiny" in token and "check" in token:
                found.add(token)
    return found


class JustCheckCoversCiTest(unittest.TestCase):
    maxDiff = None
    def test_makefile_tiny_check_names_include_the_known_targets(self):
        targets = tiny_check_targets(MAKEFILE.read_text(encoding="utf-8"))
        missing = sorted(KNOWN_TINY_CHECKS - targets)
        self.assertEqual(
            missing,
            [],
            "c/Makefile parse missed tiny check targets: " + ", ".join(missing),
        )
        self.assertIn("mimo-tiny-serve-check", targets)

    def test_check_show_and_dependency_bodies_cover_linux_ci(self):
        show = subprocess.run(
            ["just", "--show", "check"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        show_text = show.stdout
        just_text = JUSTFILE.read_text(encoding="utf-8")
        recipes = recipe_texts(just_text)
        deps = check_dependencies(show_text, just_text)
        parts = [show_text]
        missing = []
        if not deps:
            missing.append("no direct dependencies parsed for check")
        for dep in deps:
            body = recipes.get(dep)
            if body is None:
                missing.append("recipe body missing: " + dep)
                continue
            parts.append(body)
        blob = "\n".join(parts)
        for name in REQUIRED:
            if name not in blob:
                missing.append(name)
        targets = tiny_check_targets(MAKEFILE.read_text(encoding="utf-8"))
        for name in sorted(targets):
            if name not in blob:
                missing.append(name)
        python_test = recipes.get("python-test", "")
        if "COLI_OMIT_TOK_S_TIMING" not in python_test:
            missing.append("python-test body COLI_OMIT_TOK_S_TIMING")
        if ENGINE_LIST not in python_test:
            missing.append("python-test body " + ENGINE_LIST)
        if '(python-test "1")' not in show_text and 'python-test "1"' not in show_text:
            missing.append('show output python-test "1"')
        self.assertEqual(
            missing,
            [],
            "just check coverage is missing: " + ", ".join(missing),
        )


if __name__ == "__main__":
    unittest.main()
