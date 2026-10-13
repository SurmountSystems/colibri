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
        && w6144 == 0 && strcmp(r6144, "ARTIFACT_UNAVAILABLE") == 0
        && wother == 0 && strcmp(rother, "SHAPE_NO_PROGRAM") == 0;

    node[0] = '\0';
    has_node = coli_xdna_linux_device_node(node, sizeof node);
    printf("engine=kimi_k3 submit=%d node=%s reason=%s\n",
           rc, (has_node && node[0]) ? node : "-", decline);

    if(rc == 1 && strcmp(decline, "COMPLETED") == 0 && outs_match && shapes_ok)
        return 0;
    return 1;
}
