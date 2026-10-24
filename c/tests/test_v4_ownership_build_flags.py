"""Parent Makefile must rebuild build/ownership/ objects when flags change.

The ownership objects are named after the unit, not the flags, and live
across `make test-asan` (EXTRA_CFLAGS=-fsanitize=address) and a later
non-ASan `make tests/test_v4_ownership` (engine / test-c). Timestamps
cannot tell those apart, so without a record of the flags make reports
"is up to date" and links ASan objects into a non-ASan binary:

    make tests/test_v4_ownership EXTRA_CFLAGS='-fsanitize=address'
    make tests/test_v4_ownership
    -> undefined __asan_register_globals (#1702 class)

The compiler here is a stand-in that only creates its -o target, so the
test needs make and sh but no toolchain, and checks exactly what make
decides to rebuild. The sources are copied, not linked, so their
timestamps can be pinned without touching the tree.
"""
import os
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path


C_DIR = Path(__file__).resolve().parent.parent
MAKE = shutil.which("make")
UNIT = "build/ownership/COLI_V4_UNIT_CONFIG.o"

FAKE_CC = """#!/bin/sh
# Stand-in compiler: creates the -o target and compiles nothing.
# Parent Makefile AUTODEP (#1741) also requires <output>.d beside each
# ownership object; a missing .d is out of date and would rebuild every time.
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then
        : > "$2"
        case "$2" in
            *.o) : > "${2%.o}.d" ;;
        esac
        exit 0
    fi
    shift
done
exit 1
"""


@unittest.skipUnless(MAKE and os.name == "posix", "make and a POSIX shell are required")
class V4OwnershipBuildFlagsTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        shutil.copy(C_DIR / "Makefile", self.dir / "Makefile")
        shutil.copy(C_DIR / "Makefile.deepseek-v4.units",
                    self.dir / "Makefile.deepseek-v4.units")
        for name in ("deepseek_v4.c", "expert_store_registry.c",
                     "backend_xdna.c"):
            (self.dir / name).write_text("/* flag-stamp fixture */\n",
                                         encoding="utf-8")
        fake = self.dir / "fakecc.sh"
        fake.write_text(FAKE_CC, encoding="utf-8")
        self.cc = f"sh {fake}"
        past = time.time() - 1000
        for f in self.dir.iterdir():
            os.utime(f, (past, past))

    def tearDown(self):
        self._tmp.cleanup()

    def make(self, *args):
        return subprocess.run(
            [MAKE, "--no-print-directory",
             f"CC={self.cc}",
             "TRIPLET=x86_64-unknown-linux-gnu",
             *args, UNIT],
            cwd=self.dir, text=True, capture_output=True, check=False,
            timeout=120)

    def build(self, *variables):
        """Build the unit, then age everything this invocation wrote.

        Leaves every file this build wrote at one timestamp, older than now
        and newer than the sources: a later build only recompiles if make
        finds a real reason to, never because two writes landed in the same
        clock tick.
        """
        before = {}
        for f in self.dir.rglob("*"):
            if f.is_file():
                before[f] = f.stat().st_mtime
        result = self.make(*variables)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        aged = time.time() - 500
        for f in self.dir.rglob("*"):
            if f.is_file() and before.get(f) != f.stat().st_mtime:
                os.utime(f, (aged, aged))
        return "-c deepseek_v4.c" in result.stdout

    def test_same_flags_do_not_rebuild(self):
        """The guard: a fix that always recompiles would pass the tests below."""
        self.assertTrue(self.build("EXTRA_CFLAGS=-DCOLI_TEST_FLAGS_A"))
        self.assertFalse(self.build("EXTRA_CFLAGS=-DCOLI_TEST_FLAGS_A"),
                         f"{UNIT} was recompiled with unchanged flags")

    def test_changed_flags_rebuild(self):
        """EXTRA_CFLAGS is how the parent's test-asan reaches this Makefile."""
        self.assertTrue(self.build("EXTRA_CFLAGS=-DCOLI_TEST_FLAGS_A"))
        self.assertTrue(self.build("EXTRA_CFLAGS=-DCOLI_TEST_FLAGS_B"),
                        f"{UNIT} was reused after EXTRA_CFLAGS changed: the "
                        "objects of one build configuration leak into another")


if __name__ == "__main__":
    unittest.main()
