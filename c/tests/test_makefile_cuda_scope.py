"""CUDA=1 must not decorate engines that have no CUDA backend (#783).

kimi_k3.c contains no COLI_CUDA code — its GPU path is Vulkan. CUDA=1 appends
-DCOLI_CUDA to the global CFLAGS and the cudart libraries to LDFLAGS, so the
kimi_k3 target used to compile a define that matches nothing and link a runtime
it never calls. The build printed no warning, so the compile line, the linked
libraries and the exit status all said "CUDA build" while the GPUs sat idle.

This asserts the shape of the recipe rather than running a compiler, so it
works on hosts without CUDA installed.
"""
import shutil
import subprocess
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
CUDA_MARKERS = ("-DCOLI_CUDA", "-lcudart")


def recipe(target, *variables):
    """The commands make WOULD run, without running them.

    -B (always-make) as well as -n: without it an already-built target prints
    "is up to date" and no recipe at all, so the assertions below would pass
    vacuously on any tree where someone had run make first.
    """
    result = subprocess.run(["make", "-Bn", target, *variables],
                            cwd=HERE, text=True, capture_output=True,
                            check=False, timeout=120)
    return result.stdout + result.stderr


def cuda_flag_is_accepted():
    """Whether this toolchain emits a CUDA recipe at all.

    CUDA=1 is a hard error off Linux and the wording differs per platform
    ("supported only on Linux" on macOS, "On Windows use: make CUDA_DLL=1" under
    MSYS2), so matching the message is fragile — the Windows text was missed the
    first time and the job failed on an absence rather than a regression. Test
    the FACT instead: if the engine that definitely HAS a CUDA backend does not
    receive -DCOLI_CUDA, there is no recipe here worth inspecting.
    """
    return "-DCOLI_CUDA" in recipe("colibri", "CUDA=1")


@unittest.skipUnless(shutil.which("make"), "make is not installed")
@unittest.skipUnless(shutil.which("make") and cuda_flag_is_accepted(),
                     "this toolchain rejects CUDA=1 before any recipe is emitted")
class MakefileCudaScopeTest(unittest.TestCase):
    def test_kimi_k3_is_not_built_with_cuda_flags(self):
        out = recipe("kimi_k3", "CUDA=1")
        compile_lines = [l for l in out.splitlines() if "kimi_k3.c" in l]
        self.assertTrue(compile_lines, f"no kimi_k3 compile line in:\n{out}")
        for line in compile_lines:
            for marker in CUDA_MARKERS:
                self.assertNotIn(marker, line,
                                 f"kimi_k3 has no CUDA backend but the recipe "
                                 f"carries {marker}:\n{line}")

    def test_kimi_k3_says_why_cuda_was_dropped(self):
        """Silently ignoring CUDA=1 would be its own trap: say it out loud."""
        out = recipe("kimi_k3", "CUDA=1")
        self.assertIn("no CUDA backend", out)
        self.assertIn("Vulkan", out, "the message must name the path that does work")

    def test_kimi_k3_without_cuda_is_unchanged(self):
        out = recipe("kimi_k3")
        for marker in CUDA_MARKERS:
            self.assertNotIn(marker, out)
        self.assertNotIn("no CUDA backend", out,
                         "the warning must only appear when CUDA=1 was asked for")

    def test_colibri_still_gets_cuda(self):
        """The guard must be scoped to the engines that lack the backend."""
        out = recipe("colibri", "CUDA=1")
        self.assertIn("-DCOLI_CUDA", out,
                      "colibri.c does have a CUDA backend and must keep it")

    def test_libcolibri_cuda_packs_backend_object(self):
        """FFI GLM archive with CUDA=1 must include backend_cuda (open:ffi-gpu)."""
        out = recipe("libcolibri", "CUDA=1")
        self.assertIn("-DCOLI_CUDA", out,
                      "colibri.lib.o must compile with COLI_CUDA when CUDA=1")
        self.assertIn("backend_cuda", out,
                      "libcolibri.a must pack backend_cuda.o for embed CUDA")
        self.assertIn("libcolibri.a", out)

    def test_libcolibri_default_is_cpu_only(self):
        """Default feature=ffi matrix: no CUDA objects in the GLM archive."""
        out = recipe("libcolibri")
        for marker in CUDA_MARKERS:
            self.assertNotIn(marker, out,
                             f"default libcolibri must stay CPU-only; found {marker}")
        # backend_cuda.o must not be a prerequisite of the CPU archive recipe.
        ar_lines = [l for l in out.splitlines() if "libcolibri.a" in l or "ar " in l.lower()]
        joined = "\n".join(ar_lines) if ar_lines else out
        self.assertNotIn("backend_cuda", joined,
                         "CPU libcolibri must not archive backend_cuda.o")


def hip_flag_is_accepted():
    """Whether this toolchain emits a HIP recipe at all (Linux + ROCm path)."""
    # Explicit HIP_ARCH avoids rocm_agent_enumerator failures on GPU-less hosts.
    return "-DCOLI_CUDA" in recipe("libcolibri", "HIP=1", "HIP_ARCH=gfx1100")


@unittest.skipUnless(shutil.which("make"), "make is not installed")
@unittest.skipUnless(shutil.which("make") and hip_flag_is_accepted(),
                     "this toolchain rejects HIP=1 before any recipe is emitted")
class MakefileHipFfiScopeTest(unittest.TestCase):
    """FFI static lib packs HIP backend objects (feature ffi-hip / HIP=1)."""

    def test_libcolibri_hip_packs_backend_object(self):
        out = recipe("libcolibri", "HIP=1", "HIP_ARCH=gfx1100")
        self.assertIn("-DCOLI_CUDA", out,
                      "colibri.lib.o must compile with COLI_CUDA when HIP=1")
        self.assertIn("backend_cuda", out,
                      "libcolibri.a must pack backend_cuda.o for embed HIP")
        self.assertIn("libcolibri.a", out)
        # HIP path must use hipcc (or name HIP compiler tooling). Do not OR with
        # backend_cuda alone — that marker is already required above and would
        # make a CUDA-only mis-recipe under HIP=1 pass vacuously.
        self.assertTrue(
            "hipcc" in out or "HIPCC" in out,
            f"HIP=1 recipe must invoke hipcc (not only pack backend_cuda):\n{out}",
        )

    def test_libcolibri_cuda_and_hip_mutually_exclusive(self):
        result = subprocess.run(
            ["make", "-Bn", "libcolibri", "CUDA=1", "HIP=1", "HIP_ARCH=gfx1100"],
            cwd=HERE, text=True, capture_output=True, check=False, timeout=120,
        )
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0,
                            "CUDA=1 and HIP=1 together must fail")
        self.assertTrue(
            "choose CUDA=1 or HIP=1" in combined or "not both" in combined,
            f"expected mutual-exclusion error:\n{combined}",
        )


if __name__ == "__main__":
    unittest.main()
