// Optional AMD XDNA2 helper for Linux. On Linux this is the only Colibri
// binary that links XRT. c/xdna/Makefile builds it as libcoli_xdna.so. The
// host in c/backend_xdna.c dlopens that file by absolute path and does not
// link XRT.
// The Windows helper remains c/backend_xdna_helper.cpp (coli_xdna.dll).
//
// This file is NOT part of an ordinary Colibri build. A machine with no NPU,
// no XRT and no helper is a normal machine; the engine keeps its normal path.
// Nothing here records a throughput number. open/execute run only when the
// host hands over an already-selected artifact. A missing device or a missing
// artifact returns an error and does not invent an output.
//
// WHAT THIS FILE OWNS
//   XRT runtime objects, the device, the hardware context, the kernel, the
//   artifact program, the device-visible activation and output buffers, the
//   host-only copy of the caller's weight on argument group 4, the blocking submit
//   and wait, and the release of all of the above.
//
// WHAT THIS FILE MUST NEVER OWN
//   Anything that requires knowing what the operation MEANS. It is not told the
//   semantic family, the tensor, the expert, the layer, the model, the routing
//   decision, the quantisation format or the group size, and it makes no
//   selection, no fallback decision and no economic judgement. It is handed an
//   already-selected, already-integrity-verified artifact and an
//   already-prepared buffer, and it executes.
//
// Every export catches std::exception AND unknown exceptions and converts them
// to a status code. No exception may cross the C ABI.
//
// Build: c/xdna/Makefile. Requires the XRT headers and libxrt_coreutil.
// Upstream documents that library under /opt/xilinx/xrt/lib. A distro package
// may install the same soname under /usr/lib. See docs/xdna.md.

#include <cstdint>
#include <cstring>
#include <cstdio>
#include <string>
#include <vector>
#include <fstream>

#include "xrt/xrt_device.h"
#include "xrt/xrt_kernel.h"
#include "xrt/xrt_bo.h"
#include "xrt/experimental/xrt_xclbin.h"

#define COLI_XDNA_HELPER_API extern "C" __attribute__((visibility("default")))

// Must equal COLI_XDNA_ABI_VERSION in c/backend_xdna.h. Exact equality: the
// host refuses any other generation outright.
#define COLI_XDNA_HELPER_ABI 2u

// Must match the enum in backend_xdna.c. Kept as plain ints so the boundary
// carries no type of its own.
enum {
    H_OK           =  0,
    H_E_DEVICE     = -1,
    H_E_ARTIFACT   = -2,
    H_E_NOT_OPEN   = -3,
    H_E_WRAP       = -4,
    H_E_SIZE       = -5,
    H_E_DISPATCH   = -6,
    H_E_COMPLETION = -7,
    H_E_EXCEPTION  = -8
};

namespace {

struct Lane {
    bool             open = false;
    xrt::device     *dev  = nullptr;
    xrt::xclbin     *xcl  = nullptr;
    xrt::hw_context *ctx  = nullptr;
    xrt::kernel     *ker  = nullptr;
    xrt::bo         *bo_i = nullptr;   // instruction stream, group 1
    xrt::bo         *bo_a = nullptr;   // activation, host-only group 3
    xrt::bo         *bo_c = nullptr;   // output, host-only group 5
    xrt::bo         *bo_w = nullptr;   // weight, host-only group 4 (copied)
    std::vector<uint32_t> instr;
    size_t m = 0, k = 0, n = 0;
};

Lane g;
char g_err[512] = {0};

void set_err(const char *s) { std::snprintf(g_err, sizeof g_err, "%s", s ? s : ""); }

std::vector<uint32_t> load_instr(const char *p) {
    std::ifstream f(p, std::ios::binary | std::ios::ate);
    if (!f) return {};
    std::streamsize n = f.tellg();
    if (n <= 0 || (n % 4) != 0) return {};
    f.seekg(0);
    std::vector<uint32_t> v(static_cast<size_t>(n) / 4);
    f.read(reinterpret_cast<char *>(v.data()), n);
    if (!f) return {};
    return v;
}

// Release everything, in reverse construction order, exactly once each. Safe
// when nothing was ever constructed, and safe to repeat: every pointer is
// nulled as it is deleted.
void teardown() {
    delete g.bo_w; g.bo_w = nullptr;   // wrapper first: it borrows caller memory
    delete g.bo_c; g.bo_c = nullptr;
    delete g.bo_a; g.bo_a = nullptr;
    delete g.bo_i; g.bo_i = nullptr;
    delete g.ker;  g.ker  = nullptr;
    delete g.ctx;  g.ctx  = nullptr;
    delete g.xcl;  g.xcl  = nullptr;
    delete g.dev;  g.dev  = nullptr;
    g.instr.clear();
    g.m = g.k = g.n = 0;
    g.open = false;
}

} // namespace

COLI_XDNA_HELPER_API unsigned int coli_xdna_helper_abi_version(void) {
    return COLI_XDNA_HELPER_ABI;
}

COLI_XDNA_HELPER_API const char *coli_xdna_helper_last_error(void) {
    return g_err;
}

// Open the device and the already-selected artifact for one (m, k, n).
//
// The host has already verified that these bytes hash to the values the
// research programme qualified; this function does not re-decide which artifact
// to use, and could not, since it is never told what the operation is.
COLI_XDNA_HELPER_API int coli_xdna_helper_open(const char *xclbin, const char *insts,
                                               uint32_t m, uint32_t k, uint32_t n) {
    if (!xclbin || !insts || m == 0 || k == 0 || n == 0) { set_err("bad open arguments"); return H_E_SIZE; }
    try {
        teardown();                        // a re-open never layers onto old state

        g.instr = load_instr(insts);
        if (g.instr.empty()) { set_err("instruction stream missing or malformed"); return H_E_ARTIFACT; }

        g.dev = new xrt::device(static_cast<unsigned int>(0));
        g.xcl = new xrt::xclbin(std::string(xclbin));
        g.dev->register_xclbin(*g.xcl);
        g.ctx = new xrt::hw_context(*g.dev, g.xcl->get_uuid());
        g.ker = new xrt::kernel(*g.ctx, "MLIR_AIE");

        g.m = m; g.k = k; g.n = n;
        g.bo_i = new xrt::bo(*g.dev, g.instr.size() * 4, XCL_BO_FLAGS_CACHEABLE, g.ker->group_id(1));
        g.bo_a = new xrt::bo(*g.dev, static_cast<size_t>(m) * k * 2, XRT_BO_FLAGS_HOST_ONLY, g.ker->group_id(3));
        g.bo_c = new xrt::bo(*g.dev, static_cast<size_t>(m) * n * 4, XRT_BO_FLAGS_HOST_ONLY, g.ker->group_id(5));

        std::memcpy(g.bo_i->map<void *>(), g.instr.data(), g.instr.size() * 4);
        g.bo_i->sync(XCL_BO_SYNC_BO_TO_DEVICE);

        g.open = true;
        set_err("");
        return H_OK;
    } catch (const std::exception &e) {
        set_err(e.what()); teardown();
        // A device that will not open and an artifact that will not load are
        // different failures for the host: one disables the lane, the other
        // only this shape. XRT reports both as exceptions, so this cannot be
        // distinguished perfectly here; the host treats an open failure as
        // artifact-scoped, which is the narrower and therefore safer claim.
        return H_E_ARTIFACT;
    } catch (...) {
        set_err("unknown exception in open"); teardown();
        return H_E_EXCEPTION;
    }
}

// Copy the caller's 2-byte weight image into a host-only buffer on kernel
// argument group 4. The public IRON npu2 MLIR_AIE matmul (opcode 3, instr on
// group 1, buffers on groups 3, 4, and 5) is built that way. A userptr wrap
// without a group id is a different ABI and is not this kernel.
// The helper still does not interpret the elements. BF16 and int16 are both
// two bytes here; the host decides which program it opened.
COLI_XDNA_HELPER_API int coli_xdna_helper_wrap_weight(void *bf16, uint64_t bytes) {
    if (!g.open) { set_err("wrap before open"); return H_E_NOT_OPEN; }
    if (!bf16 || bytes == 0) { set_err("bad wrap arguments"); return H_E_SIZE; }
    if (bytes != static_cast<uint64_t>(g.k) * g.n * 2) { set_err("weight size does not match K*N*2"); return H_E_SIZE; }
    try {
        delete g.bo_w; g.bo_w = nullptr;
        g.bo_w = new xrt::bo(*g.dev, static_cast<size_t>(bytes), XRT_BO_FLAGS_HOST_ONLY,
                             g.ker->group_id(4));
        std::memcpy(g.bo_w->map<void *>(), bf16, static_cast<size_t>(bytes));
        g.bo_w->sync(XCL_BO_SYNC_BO_TO_DEVICE);
        set_err("");
        return H_OK;
    } catch (const std::exception &e) {
        set_err(e.what()); delete g.bo_w; g.bo_w = nullptr; return H_E_WRAP;
    } catch (...) {
        set_err("unknown exception in wrap"); delete g.bo_w; g.bo_w = nullptr; return H_E_EXCEPTION;
    }
}

// One blocking operation: stage the activation, submit, wait, read the output
// back. No worker thread, no queue, and never more than one operation
// outstanding -- the serialized V1 posture the concurrency research froze.
//
// c_f32 receives all artifact-M rows. Deciding which of them are logical output
// is the host's job, and the host copies only those.
COLI_XDNA_HELPER_API int coli_xdna_helper_execute(const void *a_bf16, uint64_t a_bytes,
                                                  void *c_f32, uint64_t c_bytes) {
    if (!g.open || !g.bo_w) { set_err("execute before open/wrap"); return H_E_NOT_OPEN; }
    if (!a_bf16 || !c_f32)  { set_err("null execute buffer"); return H_E_SIZE; }
    if (a_bytes != static_cast<uint64_t>(g.m) * g.k * 2) { set_err("activation size mismatch"); return H_E_SIZE; }
    if (c_bytes != static_cast<uint64_t>(g.m) * g.n * 4) { set_err("output size mismatch"); return H_E_SIZE; }
    try {
        std::memcpy(g.bo_a->map<void *>(), a_bf16, static_cast<size_t>(a_bytes));
        g.bo_a->sync(XCL_BO_SYNC_BO_TO_DEVICE);

        auto r = xrt::run(*g.ker);
        r.set_arg(0, static_cast<unsigned int>(3));
        r.set_arg(1, *g.bo_i);
        r.set_arg(2, g.instr.size());
        r.set_arg(3, *g.bo_a);
        // Bind the buffer object, not a scalar. A scalar overload is accepted
        // by the runtime and then completes with an output the kernel did not write.
        xrt::bo &wref = *g.bo_w;
        r.set_arg(4, wref);
        r.set_arg(5, *g.bo_c);

        r.start();
        auto st = r.wait();
        if (st != ERT_CMD_STATE_COMPLETED) {
            set_err("command did not complete");
            return H_E_COMPLETION;
        }
        g.bo_c->sync(XCL_BO_SYNC_BO_FROM_DEVICE);
        std::memcpy(c_f32, g.bo_c->map<const void *>(), static_cast<size_t>(c_bytes));
        set_err("");
        return H_OK;
    } catch (const std::exception &e) {
        set_err(e.what()); return H_E_DISPATCH;
    } catch (...) {
        set_err("unknown exception in execute"); return H_E_EXCEPTION;
    }
}

// Drop the userptr wrapper while keeping the artifact runtime open. The caller
// memory it borrowed is untouched: the engine owns it and always did.
COLI_XDNA_HELPER_API int coli_xdna_helper_release_weight(void) {
    try {
        delete g.bo_w; g.bo_w = nullptr;
        return H_OK;
    } catch (...) {
        g.bo_w = nullptr; set_err("unknown exception in release_weight"); return H_E_EXCEPTION;
    }
}

COLI_XDNA_HELPER_API void coli_xdna_helper_shutdown(void) {
    try { teardown(); } catch (...) { /* nothing useful remains to report */ }
}
