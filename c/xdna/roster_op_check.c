/* One representative model from each 2.0 roster family.
 * Prints one line per model. Exit 0 only when every line executed and matched.
 * A missing artifact is a decline, not a run.
 */
#include "backend_xdna.h"

#include <stdio.h>
#include <string.h>

static const char *k_models[] = {
    "glm-5.2-colibri",
    "glm-5.3-flash-colibri",
    "inkling-colibri",
    "kimi-k3-colibri",
    "deepseek-v4-colibri",
    "olmoe-colibri",
    "qwen-image-2.1-colibri",
    "qwen36-35b",
    "qwen38-flash-next",
    "deepseek-v4.1-flash-colibri",
    "mimo-v2.6-flash",
    "laya",
    "gliner2.5-decide"
};

int main(int argc, char **argv){
    if(argc != 3 && argc != 4){
        fprintf(stderr, "usage: roster_op_check <libcoli_xdna.so> <artifact-root> [model-id]\n");
        return 2;
    }
    int failed = 0;
    int seen = 0;
    size_t n = sizeof k_models / sizeof k_models[0];
    for(size_t i = 0; i < n; i++){
        if(argc == 4 && strcmp(k_models[i], argv[3]) != 0) continue;
        seen++;
        char reason[256];
        char node[128];
        int matched = 0;
        reason[0] = '\0';
        node[0] = '\0';
        int executed = coli_xdna_linux_roster_op(
            k_models[i], argv[1], argv[2],
            reason, sizeof reason, node, sizeof node, &matched, NULL);
        printf("model=%s executed=%d matched=%d node=%s reason=%s\n",
               k_models[i], executed, matched, node[0] ? node : "-", reason);
        if(!executed || !matched) failed = 1;
    }
    if(seen == 0){
        fprintf(stderr, "model not in the family list\n");
        return 2;
    }
    return failed ? 1 : 0;
}
