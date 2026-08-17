/* Smoke + contract tests for coli_*_visual_poll (embed visual ABI).
 *
 * Build (from c/):
 *   make libcolibri
 *   $(CC) $(CFLAGS) -I. tests/test_visual_poll_api.c -o tests/test_visual_poll_api \
 *       libcolibri.a $(LDFLAGS)
 *
 * Optional live smoke: COLIBRI_VISUAL_SMOKE_DIR=./glm_tiny ./tests/test_visual_poll_api
 *
 * Binary layouts (for Rust fixtures; match c/telemetry.h + serve parsers):
 *   EMAP cell: (tier << 6) | heat   e.g. tier=1 heat=3 -> 0x43
 *   HITS bits: little-endian in each byte, expert index i -> bit (i & 7) of byte (i >> 3)
 *   PROF fields: wall_s, prompt, completion, edisk, ewait, emm, attn, head, forwards
 */
#include "colibri_api.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails;

static void expect_i(const char *name, int got, int want) {
    if (got != want) {
        fprintf(stderr, "FAIL %s: got %d want %d\n", name, got, want);
        fails++;
    }
}

static void expect_u(const char *name, uint32_t got, uint32_t want) {
    if (got != want) {
        fprintf(stderr, "FAIL %s: got %u want %u\n", name, got, want);
        fails++;
    }
}

/* Fixed packing oracle for Rust VisualSnapshot decode (no engine needed). */
static void test_cell_packing(void) {
    /* tier 1, heat 3 -> 0x43; tier 2, heat 0 -> 0x80 */
    uint8_t a = (uint8_t)((1u << 6) | 3u);
    uint8_t b = (uint8_t)((2u << 6) | 0u);
    expect_u("emap_cell_ram_heat3", a, 0x43);
    expect_u("emap_cell_vram_heat0", b, 0x80);
    /* hits: experts 0 and 3 set in first byte */
    uint8_t bits = (uint8_t)((1u << 0) | (1u << 3));
    expect_u("hits_bits_0_and_3", bits, 0x09);
}

static void test_null_engine(void) {
    char err[128];
    ColiHwinfoSnap hw;
    ColiTiersSnap tiers;
    ColiExpertGridDims ed = {0}, hd = {0};
    size_t elen = 0, hlen = 0;
    uint64_t hseq = 0;
    ColiProfSnap prof;
    int rc = coli_glm_visual_poll(NULL, COLI_VISUAL_ALL, &hw, &tiers, &ed, NULL, 0, &elen, &hd,
                                  NULL, 0, &hlen, &hseq, &prof, err, sizeof(err));
    expect_i("null_engine", rc, -1);
}

static void test_optional_smoke(void) {
    const char *dir = getenv("COLIBRI_VISUAL_SMOKE_DIR");
    if (!dir || !dir[0]) {
        printf("skip live smoke (set COLIBRI_VISUAL_SMOKE_DIR)\n");
        return;
    }
    ColiGlmOpenOptions o = {.model_dir = dir, .cap = 8, .expert_bits = 4, .dense_bits = 8};
    ColiGlmEngine *e = NULL;
    char err[256];
    if (coli_glm_engine_open(&e, &o, err, sizeof(err)) != 0) {
        fprintf(stderr, "FAIL smoke open: %s\n", err);
        fails++;
        return;
    }
    ColiHwinfoSnap hw;
    ColiTiersSnap tiers;
    ColiExpertGridDims ed = {0}, hd = {0};
    size_t elen = 0, hlen = 0;
    uint64_t hseq = 0;
    ColiProfSnap prof;
    memset(&hw, 0, sizeof(hw));
    memset(&tiers, 0, sizeof(tiers));
    memset(&prof, 0, sizeof(prof));
    /* Query sizes first */
    int rc = coli_glm_visual_poll(e, COLI_VISUAL_ALL, &hw, &tiers, &ed, NULL, 0, &elen, &hd, NULL,
                                  0, &hlen, &hseq, &prof, err, sizeof(err));
    if (rc != 0) {
        fprintf(stderr, "FAIL smoke poll query: %d %s\n", rc, err);
        fails++;
        coli_glm_engine_destroy(e);
        return;
    }
    uint8_t *cells = elen ? (uint8_t *)malloc(elen) : NULL;
    uint8_t *bits = hlen ? (uint8_t *)malloc(hlen) : NULL;
    rc = coli_glm_visual_poll(e, COLI_VISUAL_ALL, &hw, &tiers, &ed, cells, elen, &elen, &hd, bits,
                              hlen, &hlen, &hseq, &prof, err, sizeof(err));
    if (rc != 0) {
        fprintf(stderr, "FAIL smoke poll fill: %d %s\n", rc, err);
        fails++;
    } else {
        printf("smoke ok: cores=%u emap %ux%u (%zu bytes) hits_len=%zu prof_valid=%d\n", hw.cores,
               ed.rows, ed.cols, elen, hlen, prof.valid);
        if (hw.cores == 0 && hw.ram_total_gb <= 0.0)
            fprintf(stderr, "WARN hwinfo looks empty (possible host probe gap)\n");
        /* before generate, PROF should be invalid */
        if (prof.valid != 0) {
            fprintf(stderr, "FAIL prof.valid expected 0 before generate\n");
            fails++;
        }
    }
    free(cells);
    free(bits);
    coli_glm_engine_destroy(e);
}

int main(void) {
    test_cell_packing();
    test_null_engine();
    test_optional_smoke();
    if (fails) {
        fprintf(stderr, "%d failure(s)\n", fails);
        return 1;
    }
    printf("test_visual_poll_api: ok\n");
    return 0;
}
