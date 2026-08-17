"""Make jobserver, circular colibri alias, and quant.h knob hygiene.

Named contracts (operator, 2026-08-13):

1. Independent C unit-test compile/link recipes must actually run concurrently
   when the caller (or `just check`) allows parallel make. Use the GNU make
   jobserver. Prefer passing through the user's `-j` / MAKEFLAGS rather than
   hard-forcing a huge `-j` that starts a new jobserver. Nested make must use
   `$(MAKE)` so it does not drop the jobserver. Do not parallelize steps that
   must stay serial (`make clean` before `portable`).

2. `make -C c colibri` / `make portable` must not print
   `Circular colibri <- colibri dependency dropped.`

3. `g_idot` / `g_i4s` / `g_xexp` must not be unused file-scope statics in
   `quant.h` when unit tests include that header for kernels only. Do not
   paper this over with `-Wno-unused-variable` on the tree, and do not delete
   the live engine knobs.

These tests are structural (recipe shape, dry-run, cheap compile of two TUs).
They are not a wall-clock race used as a CI gate.
"""
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


C_DIR = Path(__file__).resolve().parents[1]
REPO = C_DIR.parent
JUSTFILE = REPO / "justfile"
MAKEFILE = C_DIR / "Makefile"
QUANT_H = C_DIR / "quant.h"
COLIBRI_C = C_DIR / "colibri.c"
MAKE = shutil.which("make")
JUST = shutil.which("just")
CC = shutil.which("gcc") or shutil.which("cc")

KNOB_NAMES = ("g_idot", "g_i4s", "g_xexp")
CIRCULAR_RE = re.compile(r"Circular\s+colibri\s+<-\s+colibri", re.I)


def _read(path):
    return path.read_text(encoding="utf-8")


def _recipe_lines(text):
    for line in text.splitlines():
        if line.startswith("\t"):
            yield line


def _has_bare_make(line):
    """True if a recipe line invokes `make` without going through $(MAKE).

    Quoted usage text such as `echo "usage: make foo"` is not an invocation.
    A path like tools/make_deepseek_v4_tiny.py is not an invocation either.
    """
    stripped = line.replace("$(MAKE)", "")
    no_quotes = re.sub(r'"[^"]*"|\'[^\']*\'', "", stripped)
    return re.search(r"(^|[\s;`])make([\s]|$)", no_quotes) is not None


class HasBareMakeTests(unittest.TestCase):
    """Oracle for the jobserver scan: no product I/O."""

    def test_true_for_bare_make_recipes(self):
        self.assertTrue(_has_bare_make("\tmake clean"))
        self.assertTrue(_has_bare_make("\tmake -C foo bar"))

    def test_false_for_make_variable_quoted_usage_and_tool_path(self):
        self.assertFalse(_has_bare_make("\t$(MAKE) clean"))
        self.assertFalse(
            _has_bare_make(
                '\t@test -n "$(MODEL)" || { echo "usage: make deepseek-v4-oracle '
                'MODEL=/path/to/checkpoint" >&2; exit 2; }'
            )
        )
        self.assertFalse(_has_bare_make("\t$(PYTHON) tools/make_deepseek_v4_tiny.py"))


def _target_recipe(makefile, target):
    """Tab-indented recipe lines of the first `target:` rule."""
    lines = makefile.splitlines()
    header = re.compile(rf"^{re.escape(target)}\s*:")
    out = []
    in_recipe = False
    for line in lines:
        if not in_recipe:
            if header.match(line):
                in_recipe = True
            continue
        if line.startswith("\t"):
            out.append(line)
            continue
        if line.startswith("#") or line.strip() == "":
            # comments / blanks between recipe lines still belong to the rule
            # only if more tabs follow; stop on the next rule header.
            continue
        break
    return out


@unittest.skipUnless(MAKE, "make is required")
class MakefileJobserverTests(unittest.TestCase):
    def test_no_global_notparallel(self):
        """A global .NOTPARALLEL would serialize every independent recipe."""
        text = _read(MAKEFILE)
        self.assertIsNone(
            re.search(r"^\.NOTPARALLEL\b", text, re.M),
            "c/Makefile must not globally disable make job parallelism",
        )

    def test_check_uses_recursive_make_and_keeps_clean_serial(self):
        recipe = _target_recipe(_read(MAKEFILE), "check")
        joined = "\n".join(recipe)
        self.assertTrue(recipe, "check recipe missing")
        for line in recipe:
            self.assertFalse(
                _has_bare_make(line),
                f"check recipe drops the jobserver via bare make:\n{line}",
            )
        # clean must finish before portable; portable before test.
        self.assertIn("$(MAKE) clean", joined)
        self.assertIn("$(MAKE) portable", joined)
        self.assertIn("$(MAKE) test", joined)
        bodies = [line.strip() for line in recipe if line.strip()]
        self.assertGreaterEqual(len(bodies), 3, f"unexpected check recipe:\n{joined}")
        self.assertEqual(bodies[0], "$(MAKE) clean")
        self.assertEqual(bodies[1], "$(MAKE) portable")
        self.assertEqual(bodies[2], "$(MAKE) test")

    def test_portable_and_nested_recipes_use_make_variable(self):
        text = _read(MAKEFILE)
        portable = _target_recipe(text, "portable")
        self.assertTrue(portable, "portable recipe missing")
        joined = "\n".join(portable)
        self.assertIn("$(MAKE)", joined)
        for line in portable:
            self.assertFalse(
                _has_bare_make(line),
                f"portable recipe drops the jobserver via bare make:\n{line}",
            )
        for line in _recipe_lines(text):
            if line.lstrip().startswith("#"):
                continue
            self.assertFalse(
                _has_bare_make(line),
                f"recipe line invokes bare make (drops jobserver):\n{line}",
            )

    def test_unit_test_binaries_are_independent_targets(self):
        text = _read(MAKEFILE)
        self.assertIn("test-c: $(TEST_BINS)", text)
        rules = re.findall(r"^tests/test_[a-z0-9_]+\$\(EXE\):", text, re.M)
        self.assertGreaterEqual(
            len(rules),
            10,
            "independent tests/test_*$(EXE) rules are what -j can compile in parallel",
        )

    def test_cflags_do_not_blanket_unused_variable(self):
        text = _read(MAKEFILE)
        self.assertNotIn(
            "-Wno-unused-variable",
            text,
            "do not paper over unused knobs with -Wno-unused-variable",
        )


@unittest.skipUnless(JUST, "just is required")
class JustCheckJobserverTests(unittest.TestCase):
    def _isolated_env(self, overlay=None):
        """Drop parent make job flags, then apply a per-test MAKEFLAGS overlay.

        `just check` exports MAKEFLAGS with -j and --jobserver-auth into
        `make test-python`. Copying os.environ as-is would hide the default -jN
        path these tests exist to lock.
        """
        env = os.environ.copy()
        for key in ("MAKEFLAGS", "MFLAGS", "GNUMAKEFLAGS"):
            env.pop(key, None)
        if overlay:
            env.update(overlay)
        return env

    def _dry(self, *args, env=None):
        result = subprocess.run(
            [JUST, "-n", *args],
            cwd=REPO,
            text=True,
            capture_output=True,
            check=False,
            timeout=60,
            env=self._isolated_env(env),
        )
        combined = result.stdout + result.stderr
        self.assertEqual(
            result.returncode,
            0,
            f"just -n {' '.join(args)} failed:\n{combined}",
        )
        return combined

    def _make_jobs_value(self):
        result = subprocess.run(
            [JUST, "--evaluate", "make_jobs"],
            cwd=REPO,
            text=True,
            capture_output=True,
            check=False,
            timeout=60,
            env=self._isolated_env(),
        )
        self.assertEqual(
            result.returncode,
            0,
            f"just --evaluate make_jobs failed:\n{result.stdout}{result.stderr}",
        )
        return int(result.stdout.strip())

    def _assert_default_jobs(self, out, make_goal):
        match = re.search(
            rf"make\s+-C\s+c\s+{re.escape(make_goal)}\s+-j([1-9][0-9]*)",
            out,
        )
        self.assertIsNotNone(
            match,
            f"just must pass a positive -j to make {make_goal}; got:\n{out}",
        )
        n = int(match.group(1))
        expected = self._make_jobs_value()
        self.assertEqual(
            n,
            expected,
            f"-jN must match just --evaluate make_jobs ({expected}); got {n} in:\n{out}",
        )
        self.assertGreaterEqual(n, 1)
        cpus = os.cpu_count()
        if cpus is not None and cpus > 1:
            self.assertGreater(
                n,
                1,
                f"host reports {cpus} CPUs; a hardcoded -j1 would serialize "
                f"independent compiles",
            )

    def test_just_c_check_default_passes_dash_j(self):
        """just check / c-check must not invoke make with implicit -j1."""
        out = self._dry("c-check")
        self._assert_default_jobs(out, "check")

    def test_just_c_check_honors_explicit_make_jobs(self):
        out = self._dry("--set", "make_jobs", "3", "c-check")
        self.assertRegex(
            out,
            r"make\s+-C\s+c\s+check\s+-j3\b",
            f"just --set make_jobs 3 c-check must pass -j3; got:\n{out}",
        )

    def test_just_c_check_does_not_override_caller_makeflags(self):
        """An existing -j / jobserver in MAKEFLAGS must not get a new -jN."""
        out = self._dry("c-check", env={"MAKEFLAGS": "-j2"})
        self.assertRegex(
            out,
            r"make\s+-C\s+c\s+check\s*$",
            f"MAKEFLAGS=-j2 just c-check must not start a new jobserver; got:\n{out}",
        )
        self.assertIsNone(
            re.search(r"make\s+-C\s+c\s+check\s+-j", out),
            f"caller -j was overridden:\n{out}",
        )

    def test_just_c_check_does_not_override_jobserver_auth(self):
        out = self._dry(
            "c-check",
            env={"MAKEFLAGS": "--jobserver-auth=fifo:/tmp/coli-fake-jobserver"},
        )
        self.assertRegex(
            out,
            r"make\s+-C\s+c\s+check\s*$",
            f"jobserver MAKEFLAGS must still invoke make -C c check; got:\n{out}",
        )
        self.assertIsNone(
            re.search(r"make\s+-C\s+c\s+check\s+-j", out),
            f"existing jobserver was overridden with a new -j:\n{out}",
        )

    def test_just_c_test_default_passes_dash_j(self):
        out = self._dry("c-test")
        self._assert_default_jobs(out, "test-c")


@unittest.skipUnless(MAKE, "make is required")
class MakefileCircularColibriTests(unittest.TestCase):
    def _dry(self, *args):
        # -B: after portable / make check, colibri.exe can already exist and
        # be up to date. Without always-make, `make -n colibri` prints
        # "Nothing to be done" and the -o colibri.exe lock is vacuously missed.
        result = subprocess.run(
            [MAKE, "--no-print-directory", "-B", "-n", *args],
            cwd=C_DIR,
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
        return result.stdout + result.stderr, result.returncode

    def test_make_colibri_has_no_circular_dependency(self):
        out, rc = self._dry("colibri")
        self.assertEqual(rc, 0, f"make -n colibri failed:\n{out}")
        self.assertIsNone(
            CIRCULAR_RE.search(out),
            f"make -n colibri printed a circular self-edge:\n{out}",
        )

    def test_make_portable_has_no_circular_dependency(self):
        out, rc = self._dry("portable")
        self.assertEqual(rc, 0, f"make -n portable failed:\n{out}")
        self.assertIsNone(
            CIRCULAR_RE.search(out),
            f"make -n portable printed a circular self-edge:\n{out}",
        )

    def test_linux_named_binary_has_no_self_edge(self):
        """On Unix EXE is empty, so `colibri: colibri` is the self-edge."""
        out, rc = self._dry("colibri", "TRIPLET=x86_64-unknown-linux-gnu")
        self.assertEqual(rc, 0, f"make -n colibri (linux triplet) failed:\n{out}")
        self.assertIsNone(
            CIRCULAR_RE.search(out),
            f"linux colibri alias is a self-edge:\n{out}",
        )

    def test_windows_colibri_alias_is_not_a_self_edge(self):
        """On Windows EXE=.exe, so `colibri` is a phony alias to colibri.exe."""
        out, rc = self._dry("colibri", "TRIPLET=x86_64-w64-mingw32")
        self.assertEqual(rc, 0, f"make -n colibri (mingw triplet) failed:\n{out}")
        self.assertIsNone(
            CIRCULAR_RE.search(out),
            f"windows colibri alias is a self-edge:\n{out}",
        )
        self.assertIn(
            "-o colibri.exe",
            out,
            f"windows dry-run must still compile colibri.exe; got:\n{out}",
        )
        self.assertNotRegex(
            out,
            r"No rule to make target [`']colibri[`']",
            f"windows `colibri` phony alias must still exist; got:\n{out}",
        )


@unittest.skipUnless(CC, "a C compiler is required")
class QuantHeaderKnobTests(unittest.TestCase):
    def test_quant_h_does_not_define_engine_knob_statics(self):
        text = _read(QUANT_H)
        for name in KNOB_NAMES:
            self.assertIsNone(
                re.search(rf"\bstatic\s+int\s+{name}\b", text),
                f"{name} must not be a file-scope static in quant.h "
                f"(every kernel-only TU compiles an unused object)",
            )

    def test_colibri_c_still_owns_the_live_knobs(self):
        """Do not delete the engine knobs; they stay live on the colibri TU."""
        text = _read(COLIBRI_C)
        for name in KNOB_NAMES:
            self.assertIsNotNone(
                re.search(rf"\b{name}\b", text),
                f"{name} must remain a live engine knob in colibri.c",
            )
        self.assertIn('getenv("IDOT")', text)
        self.assertIn('getenv("I4S")', text)
        self.assertIn('getenv("XEXP")', text)

    def _compile_kernel_tu(self, src_name):
        src = C_DIR / "tests" / src_name
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "tu.o"
            result = subprocess.run(
                [
                    CC,
                    "-Wall",
                    "-Wextra",
                    "-Wno-unused-parameter",
                    "-Wno-misleading-indentation",
                    "-Wno-unused-function",
                    "-Werror=unused-variable",
                    "-c",
                    str(src),
                    "-o",
                    str(out),
                ],
                cwd=C_DIR,
                text=True,
                capture_output=True,
                check=False,
                timeout=120,
            )
            combined = result.stdout + result.stderr
            return result.returncode, combined

    def test_e8_kernel_tu_has_no_unused_knob_warnings(self):
        rc, out = self._compile_kernel_tu("test_e8_kernel.c")
        for name in KNOB_NAMES:
            self.assertNotIn(
                name,
                out,
                f"test_e8_kernel.c still mentions unused {name}:\n{out}",
            )
        self.assertEqual(
            rc,
            0,
            f"test_e8_kernel.c must compile without unused-variable errors:\n{out}",
        )

    def test_fp8_passthrough_tu_has_no_unused_knob_warnings(self):
        rc, out = self._compile_kernel_tu("test_fp8_passthrough.c")
        for name in KNOB_NAMES:
            self.assertNotIn(name, out, f"unused {name} still warned:\n{out}")
        self.assertEqual(
            rc,
            0,
            f"test_fp8_passthrough.c must compile without unused-variable "
            f"errors:\n{out}",
        )


if __name__ == "__main__":
    unittest.main()
