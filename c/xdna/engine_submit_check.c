#include "backend_xdna.h"

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

int main(int argc, char **argv){
    int32_t out[4096];
    int16_t A[64 * 64];
    int16_t B[64 * 64];
    int32_t R[64 * 64];
    char decline[256];
    char node[128];
    char r64[64];
    char r6144[64];
    char rother[64];
    const char *model_id;
    unsigned tag;
    const unsigned M = 64, K = 64, N = 64;
    int rc;
    int outs_match;
    int w64, w6144, wother;
    int shapes_ok;
    int has_node;

    if(argc != 3){
        fprintf(stderr, "usage: engine_submit_check <helper> <artifact-root>\n");
        return 2;
    }
    coli_xdna_test_set_helper_path(argv[1]);
    coli_xdna_test_set_artifact_root(argv[2]);

    memset(out, 0x5a, sizeof out);
    decline[0] = '\0';
    rc = coli_xdna_engine_qualified_submit("kimi_k3", out, decline, sizeof decline);

    model_id = coli_xdna_engine_roster_id("kimi_k3");
    tag = 1u;
    for(const char *p = model_id; p && *p; p++) tag = tag * 33u + (unsigned char)*p;
    tag = (tag % 5u) + 1u;

    for(unsigned i = 0; i < M; i++){
        for(unsigned k = 0; k < K; k++)
            A[(size_t)i * K + k] = (int16_t)((i + k + tag) % 7u);
    }
    for(unsigned k = 0; k < K; k++){
        for(unsigned j = 0; j < N; j++)
            B[(size_t)k * N + j] = (int16_t)((k + 2u * j + tag) % 5u);
    }
    for(unsigned i = 0; i < M; i++){
        for(unsigned j = 0; j < N; j++){
            int32_t acc = 0;
            for(unsigned k = 0; k < K; k++)
                acc += (int32_t)A[(size_t)i * K + k] * (int32_t)B[(size_t)k * N + j];
            R[(size_t)i * N + j] = acc;
        }
    }

    outs_match = 1;
    for(size_t i = 0; i < 4096; i++){
        if(out[i] != R[i]){ outs_match = 0; break; }
    }

    w64 = coli_xdna_shape_would_run(64, 64, r64, sizeof r64);
    w6144 = coli_xdna_shape_would_run(6144, 2048, r6144, sizeof r6144);
    wother = coli_xdna_shape_would_run(7168, 3072, rother, sizeof rother);
    shapes_ok = w64 == 1
        && w6144 == 1 && strcmp(r6144, "TILED_64") == 0
        && wother == 1 && strcmp(rother, "TILED_64") == 0;

    node[0] = '\0';
    has_node = coli_xdna_linux_device_node(node, sizeof node);
    printf("engine=kimi_k3 submit=%d node=%s reason=%s\n",
           rc, (has_node && node[0]) ? node : "-", decline);

    /* Expert GEMM through the same 64^3 program. A missing dedicated F3
     * kernel is not a decline when that program is present. */
    {
        const int I = 6144, O = 2048, S = 1, gs = 64;
        const int rb = (I + 1) / 2, ng = (I + gs - 1) / gs;
        unsigned char *q4 = (unsigned char *)malloc((size_t)O * (size_t)rb);
        float *sc = (float *)malloc((size_t)O * (size_t)ng * sizeof(float));
        float *x = (float *)malloc((size_t)S * (size_t)I * sizeof(float));
        float *y = (float *)malloc((size_t)S * (size_t)O * sizeof(float));
        ColiXdnaPrepared *slot = NULL;
        int tiled = 0;
        size_t zi;
        if(!q4 || !sc || !x || !y){
            fprintf(stderr, "tiled expert alloc failed\n");
            free(q4); free(sc); free(x); free(y);
            return 1;
        }
        for(zi = 0; zi < (size_t)O * (size_t)rb; zi++)
            q4[zi] = (unsigned char)(zi * 17u);
        for(zi = 0; zi < (size_t)O * (size_t)ng; zi++) sc[zi] = 0.25f;
        for(zi = 0; zi < (size_t)S * (size_t)I; zi++)
            x[zi] = 0.01f * (float)((int)(zi % 13u) - 6);
        for(zi = 0; zi < (size_t)S * (size_t)O; zi++) y[zi] = -123456.f;
        coli_xdna_set_explicit_enabled(1);
        coli_xdna_test_set_force_execution(1);
        tiled = coli_xdna_try_matmul(COLI_XDNA_FAMILY_MOE_SHARED_GATE_UP, &slot,
                                     4, q4, sc, I, O, gs, 0, y, x, S);
        printf("expert_tiled K=6144 N=2048 handled=%d reason=%s y0=%g\n",
               tiled, coli_xdna_decline_reason(), (double)y[0]);
        coli_xdna_prepared_release(&slot);
        free(q4); free(sc); free(x); free(y);
        if(tiled != 1) return 1;
    }

    if(rc == 1 && strcmp(decline, "COMPLETED") == 0 && outs_match && shapes_ok)
        return 0;
    return 1;
}
