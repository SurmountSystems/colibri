/* Public embed API for Colibrì product engines (experimental).
 *
 * Family-specific entry points share the same call shape as DeepSeek V4:
 * open → (optional size summary) → generate with token callback → destroy.
 *
 * Build static libs without CLI main:
 *   make -C c libcolibri             # COLIBRI_NO_MAIN
 *   make -C c libkimi_k3             # KIMI_NO_MAIN
 *   make -C c libinkling             # INKLING_NO_MAIN (CPU only)
 *   make -f Makefile.deepseek-v4 libdeepseek-v4   # COLI_V4_SKIP_GENERATE_MAIN
 *
 * DeepSeek V4 symbols remain in deepseek_v4.h (coli_v4_*).
 */
#ifndef COLIBRI_API_H
#define COLIBRI_API_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- shared token callback (non-zero return = stop when supported) ---- */
typedef int (*ColiTokenFn)(void *user_data, int token, float logit,
                           int position, int ordinal);

/* ---- model size summary (raw bytes) ---- */
typedef struct {
    uint64_t disk_bytes;       /* total weight files on disk */
    uint64_t dense_bytes;      /* 0 if unknown at open */
    uint64_t expert_bytes;     /* 0 if unknown at open */
    uint64_t param_count;      /* 0 if not declared in config */
    int has_param_count;
    char family[32];           /* "glm" / "kimi" / "inkling" / "deepseek_v4" */
    char engine_id[32];        /* "colibri" / "kimi_k3" / "inkling" / "deepseek_v4" */
} ColiModelSizeSummary;

/* Probe model dir weights without loading decode state. */
int coli_model_size_probe(const char *model_dir, ColiModelSizeSummary *out,
                          char *error, size_t error_size);

/* Unix niceness for in-process compute / OpenMP team. Must match
 * ENGINE_CHILD_NICE in colibri-sys process_priority.rs (10).
 * Linux: setpriority(PRIO_PROCESS, 0, nice) on the calling thread and on
 * each OpenMP team member. Do not call from the UI thread. */
#define COLI_COMPUTE_NICE 10
int coli_nice_compute_threads(int nice);

/* Cooperative embed stop for in-process generate/prefill. Does not take a
 * host mutex. Checked in spec_decode, the default-path prefill layer loop,
 * and leftover skip after a chunk-loop break. */
void coli_embed_request_stop(void);
void coli_embed_clear_stop(void);
int coli_embed_should_stop(void);
/* 1 when leftover layers_forward should run (remaining tokens, not stopped). */
int coli_prefill_should_run_leftover(int remaining_tokens);

/* ======================== GLM (colibri) ======================== */

typedef struct ColiGlmEngine ColiGlmEngine;

typedef struct {
    const char *model_dir; /* required; copied */
    int cap;               /* expert cache slots; 0 => 64 */
    int expert_bits;       /* 0 => 4 */
    int dense_bits;        /* 0 => 8 */
} ColiGlmOpenOptions;

typedef struct {
    int max_new_tokens; /* required */
} ColiGlmGenerateOptions;

int coli_glm_engine_open(ColiGlmEngine **engine, const ColiGlmOpenOptions *options,
                         char *error, size_t error_size);
void coli_glm_engine_destroy(ColiGlmEngine *engine);
void coli_glm_engine_size(const ColiGlmEngine *engine, ColiModelSizeSummary *out);

int coli_glm_generate(ColiGlmEngine *engine, const char *prompt, size_t prompt_len,
                      const ColiGlmGenerateOptions *options, ColiTokenFn on_token,
                      void *user_data, char *error, size_t error_size);

/* Greedy decode from prompt token ids (no tokenizer). Used for tiny oracle /
 * process↔FFI parity: same path as CLI free-generate with ref_glm.json
 * prompt_ids. Forces temperature 0 for the call duration. */
int coli_glm_generate_ids(ColiGlmEngine *engine, const int *prompt_ids, int n_prompt,
                          const ColiGlmGenerateOptions *options, ColiTokenFn on_token,
                          void *user_data, char *error, size_t error_size);

/* ---- Visual telemetry poll (mirror c/telemetry.h + mux PROF; no stdout) ----
 *
 * Prefer poll over inventing an in-process stdout mux. Hosts call this on a
 * timer (native ~500ms) or after generate. Layouts match the SERVE line protocol
 * so Rust can reuse ServeClient / visual.rs decode (binary form, not hex lines).
 *
 * want bits:
 *   COLI_VISUAL_HWINFO  host CPU/RAM/GPU snapshot
 *   COLI_VISUAL_TIERS   expert counts per tier + resident GB
 *   COLI_VISUAL_EMAP    expert map cells: one u8 per expert, row-major
 *                       cell = (tier << 6) | heat  (tier: 0 disk, 1 RAM, 2 VRAM;
 *                       heat: log2-bucketed usage, 0..63)
 *   COLI_VISUAL_HITS    hit bitmap since previous successful HITS poll/emit:
 *                       ceil(rows*cols/8) bytes, little-endian bits in each byte.
 *                       Destructive: clears the engine hit marks on success.
 *   COLI_VISUAL_PROF    last completed generate turn phase timings
 *
 * Return codes:
 *   0  success (including empty/zero data when nothing has run yet)
 *  -1  bad args / engine not open
 *  -2  caller buffer too small (needed length written to *emap_cells_len /
 *      *hits_bits_len; dims filled when possible)
 *
 * Optional out pointers may be NULL when the corresponding want bit is clear.
 * For EMAP/HITS: pass cells/bits NULL to query needed size only (still 0).
 * STOP remains cooperative via ColiTokenFn non-zero return; no mux multi-slot
 * STOP on this path.
 */
#define COLI_VISUAL_HWINFO (1u << 0)
#define COLI_VISUAL_TIERS  (1u << 1)
#define COLI_VISUAL_EMAP   (1u << 2)
#define COLI_VISUAL_HITS   (1u << 3)
#define COLI_VISUAL_PROF   (1u << 4)
#define COLI_VISUAL_ALL    0xffffffffu

typedef struct {
    uint32_t cores;
    double ram_total_gb;
    double ram_avail_gb;
    uint32_t gpus;
    double vram_total_gb;
    char cpu[128];
    char gpu[128];
} ColiHwinfoSnap;

typedef struct {
    uint32_t vram_experts;
    uint32_t ram_experts;
    uint32_t disk_experts;
    double vram_gb;
    double ram_gb;
} ColiTiersSnap;

/* Sparse MoE layers (+MTP when present) × n_experts — same as EMAP/HITS lines. */
typedef struct {
    uint32_t rows;
    uint32_t cols;
} ColiExpertGridDims;

/* PROF <wall_s> <prompt> <completion> <edisk> <ewait> <emm> <attn> <head> <n_fw> */
typedef struct {
    double wall_s;
    uint32_t prompt_tokens;
    uint32_t completion_tokens;
    double expert_disk_s;
    double expert_wait_s;
    double expert_matmul_s;
    double attention_s;
    double lm_head_s;
    uint64_t forwards;
    uint64_t seq; /* increments each completed generate that records PROF */
    int valid;    /* 1 if at least one generate completed with PROF */
} ColiProfSnap;

int coli_glm_visual_poll(ColiGlmEngine *engine, uint32_t want, ColiHwinfoSnap *hwinfo,
                         ColiTiersSnap *tiers, ColiExpertGridDims *emap_dims,
                         uint8_t *emap_cells, size_t emap_cells_cap, size_t *emap_cells_len,
                         ColiExpertGridDims *hits_dims, uint8_t *hits_bits,
                         size_t hits_bits_cap, size_t *hits_bits_len, uint64_t *hits_seq,
                         ColiProfSnap *prof, char *error, size_t error_size);

/* ======================== Kimi K3 ======================== */

typedef struct ColiKimiEngine ColiKimiEngine;

typedef struct {
    const char *model_dir; /* required; copied */
    int n_layers;          /* 0 => all */
} ColiKimiOpenOptions;

typedef struct {
    int max_new_tokens; /* required */
} ColiKimiGenerateOptions;

int coli_kimi_engine_open(ColiKimiEngine **engine, const ColiKimiOpenOptions *options,
                          char *error, size_t error_size);
void coli_kimi_engine_destroy(ColiKimiEngine *engine);
void coli_kimi_engine_size(const ColiKimiEngine *engine, ColiModelSizeSummary *out);

int coli_kimi_generate(ColiKimiEngine *engine, const char *prompt, size_t prompt_len,
                       const ColiKimiGenerateOptions *options, ColiTokenFn on_token,
                       void *user_data, char *error, size_t error_size);

/* Visual poll stub: returns 0 with empty/zero snapshots until Kimi fill lands.
 * Does not crash link; host should treat valid=0 / rows=0 as no telemetry. */
int coli_kimi_visual_poll(ColiKimiEngine *engine, uint32_t want, ColiHwinfoSnap *hwinfo,
                          ColiTiersSnap *tiers, ColiExpertGridDims *emap_dims,
                          uint8_t *emap_cells, size_t emap_cells_cap, size_t *emap_cells_len,
                          ColiExpertGridDims *hits_dims, uint8_t *hits_bits,
                          size_t hits_bits_cap, size_t *hits_bits_len, uint64_t *hits_seq,
                          ColiProfSnap *prof, char *error, size_t error_size);

/* ======================== Inkling ======================== */

typedef struct ColiInkEngine ColiInkEngine;

typedef struct {
    const char *model_dir; /* required; copied */
    int cap;               /* expert cache slots/layer; 0 => auto from free RAM */
    int bits;              /* expert quant bits; 0 => f32 (or container) */
} ColiInkOpenOptions;

typedef struct {
    int max_new_tokens; /* required; default 32 if <= 0 */
} ColiInkGenerateOptions;

int coli_ink_engine_open(ColiInkEngine **engine, const ColiInkOpenOptions *options,
                         char *error, size_t error_size);
void coli_ink_engine_destroy(ColiInkEngine *engine);
void coli_ink_engine_size(const ColiInkEngine *engine, ColiModelSizeSummary *out);

int coli_ink_generate(ColiInkEngine *engine, const char *prompt, size_t prompt_len,
                      const ColiInkGenerateOptions *options, ColiTokenFn on_token,
                      void *user_data, char *error, size_t error_size);

/* Visual poll stub: returns 0 with empty/zero snapshots until Inkling fill lands. */
int coli_ink_visual_poll(ColiInkEngine *engine, uint32_t want, ColiHwinfoSnap *hwinfo,
                         ColiTiersSnap *tiers, ColiExpertGridDims *emap_dims,
                         uint8_t *emap_cells, size_t emap_cells_cap, size_t *emap_cells_len,
                         ColiExpertGridDims *hits_dims, uint8_t *hits_bits,
                         size_t hits_bits_cap, size_t *hits_bits_len, uint64_t *hits_seq,
                         ColiProfSnap *prof, char *error, size_t error_size);

#ifdef __cplusplus
}
#endif

#endif /* COLIBRI_API_H */
