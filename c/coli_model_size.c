/* Shared model-dir size probe for embed API (weights only: *.safetensors). */
#include "colibri_api.h"

#include <dirent.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

static int ends_with(const char *name, const char *suffix) {
    size_t n = strlen(name), s = strlen(suffix);
    return n >= s && strcmp(name + n - s, suffix) == 0;
}

static void set_err(char *error, size_t error_size, const char *msg) {
    if (!error || error_size == 0) return;
    snprintf(error, error_size, "%s", msg);
}

int coli_model_size_probe(const char *model_dir, ColiModelSizeSummary *out,
                          char *error, size_t error_size) {
    if (!model_dir || !out) {
        set_err(error, error_size, "model_dir and out are required");
        return -1;
    }
    memset(out, 0, sizeof(*out));
    DIR *d = opendir(model_dir);
    if (!d) {
        set_err(error, error_size, "cannot open model_dir");
        return -1;
    }
    uint64_t total = 0;
    struct dirent *ent;
    char path[4096];
    while ((ent = readdir(d)) != NULL) {
        if (!ends_with(ent->d_name, ".safetensors")) continue;
        if (snprintf(path, sizeof(path), "%s/%s", model_dir, ent->d_name) >=
            (int)sizeof(path))
            continue;
        struct stat st;
        if (stat(path, &st) != 0) continue;
        if (S_ISREG(st.st_mode)) total += (uint64_t)st.st_size;
    }
    closedir(d);
    out->disk_bytes = total;
    if (total == 0) {
        set_err(error, error_size, "no safetensors weights found");
        return -1;
    }
    return 0;
}
