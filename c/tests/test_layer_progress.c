/* test_layer_progress.c -- layer-progress stderr tag in layers_forward_rows_range.
 *
 * Prefill S>=8 still prints [prefill]. Decode S=1 and S=2 print [decode] on the
 * same cadence (every 4th layer and last). S<1 is silent. Tiny unit around that
 * condition; does not compile the engine. Source lockstep checks colibri.c holds
 * both fprintf lines so this file cannot drift from the product. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;

static void check(int ok, const char *what){
    if(!ok){ fprintf(stderr, "FAIL %s\n", what); failures++; }
}

/* Mirrors c/colibri.c layers_forward_rows_range. Keep in lockstep with the
 * two fprintfs; the source scan below refuses a silent product. */
static const char *coli_layer_progress_tag(int S, int i, int layer_end){
    if(S>=8 && (i%4==0 || i==layer_end-1))
        return "prefill";
    else if(S>=1 && (i%4==0 || i==layer_end-1))
        return "decode";
    return NULL;
}

static FILE *open_colibri(void){
    const char *here = __FILE__;
    char from_src[1024];
    from_src[0] = 0;
    { size_t n = strlen(here);
      if(n+16 < sizeof from_src){
          memcpy(from_src, here, n+1);
          char *slash = strrchr(from_src, '/');
          if(slash){
              *slash = 0;
              strncat(from_src, "/../colibri.c", sizeof from_src - strlen(from_src) - 1);
          } else {
              from_src[0] = 0;
          }
      }
    }
    const char *cands[] = {
        "colibri.c",
        "../colibri.c",
        "c/colibri.c",
        from_src,
        NULL
    };
    for(int i=0; cands[i]; i++){
        if(!cands[i][0]) continue;
        FILE *f = fopen(cands[i], "r");
        if(f) return f;
    }
    return NULL;
}

int main(void){
    const int n = 78;

    /* First decode: S=1, 78 layers. Prints on i%4==0 and last (i=77). */
    check(coli_layer_progress_tag(1, 0, n) && strcmp(coli_layer_progress_tag(1, 0, n), "decode")==0,
          "S=1 layer 1 is [decode]");
    check(coli_layer_progress_tag(1, 1, n)==NULL, "S=1 layer 2 is silent");
    check(coli_layer_progress_tag(1, 3, n)==NULL, "S=1 layer 4 is silent");
    check(coli_layer_progress_tag(1, 4, n) && strcmp(coli_layer_progress_tag(1, 4, n), "decode")==0,
          "S=1 layer 5 is [decode]");
    check(coli_layer_progress_tag(1, 76, n) && strcmp(coli_layer_progress_tag(1, 76, n), "decode")==0,
          "S=1 layer 77 (i%4==0) is [decode]");
    check(coli_layer_progress_tag(1, 77, n) && strcmp(coli_layer_progress_tag(1, 77, n), "decode")==0,
          "S=1 last layer is [decode]");

    /* MTP decode batch S=2 uses the same tag and cadence. */
    check(coli_layer_progress_tag(2, 0, n) && strcmp(coli_layer_progress_tag(2, 0, n), "decode")==0,
          "S=2 layer 1 is [decode]");
    check(coli_layer_progress_tag(2, 1, n)==NULL, "S=2 layer 2 is silent");
    check(coli_layer_progress_tag(2, 77, n) && strcmp(coli_layer_progress_tag(2, 77, n), "decode")==0,
          "S=2 last layer is [decode]");

    /* S=7 is still decode (below the prefill gate). */
    check(coli_layer_progress_tag(7, 0, n) && strcmp(coli_layer_progress_tag(7, 0, n), "decode")==0,
          "S=7 layer 1 is [decode]");

    /* Prefill S>=8 stays [prefill] on the same cadence. */
    check(coli_layer_progress_tag(8, 0, n) && strcmp(coli_layer_progress_tag(8, 0, n), "prefill")==0,
          "S=8 layer 1 is [prefill]");
    check(coli_layer_progress_tag(8, 1, n)==NULL, "S=8 layer 2 is silent");
    check(coli_layer_progress_tag(23, 77, n) && strcmp(coli_layer_progress_tag(23, 77, n), "prefill")==0,
          "S=23 last layer is [prefill]");
    check(coli_layer_progress_tag(23, 76, n) && strcmp(coli_layer_progress_tag(23, 76, n), "prefill")==0,
          "S=23 layer 77 is [prefill]");

    /* S<1 is silent even on a cadence tick. */
    check(coli_layer_progress_tag(0, 0, n)==NULL, "S=0 layer 1 is silent");
    check(coli_layer_progress_tag(0, 77, n)==NULL, "S=0 last layer is silent");

    /* Product fprintf format: stderr tee keeps lines that start with '['. */
    { char line[128];
      snprintf(line, sizeof line, "[decode] layer %d/%d · %d token · +%.2fs", 1, 78, 1, 0.0);
      check(line[0]=='[', "decode banner starts with [");
      check(strncmp(line, "[decode] layer 1/78", 19)==0, "decode banner shape");
      snprintf(line, sizeof line, "[prefill] layer %d/%d · %d token · +%.2fs", 78, 78, 23, 83.61);
      check(strncmp(line, "[prefill] layer 78/78", 21)==0, "prefill banner shape");
      check(strstr(line, "[decode]")==NULL, "prefill line is not [decode]");
    }

    /* Source lockstep: both fprintfs live in layers_forward_rows_range. */
    { FILE *f = open_colibri();
      check(f!=NULL, "open colibri.c");
      if(f){
          char *buf = NULL; size_t nread;
          fseek(f, 0, SEEK_END);
          { long sz = ftell(f); if(sz<0) sz = 0; nread = (size_t)sz; }
          rewind(f);
          buf = (char*)malloc(nread+1);
          check(buf!=NULL, "malloc colibri.c");
          if(buf){
              size_t got = fread(buf, 1, nread, f);
              buf[got] = 0;
              check(strstr(buf, "layers_forward_rows_range")!=NULL, "function present");
              check(strstr(buf, "fprintf(stderr,\"[prefill] layer %d/%d")!=NULL,
                    "prefill fprintf present");
              check(strstr(buf, "fprintf(stderr,\"[decode] layer %d/%d")!=NULL,
                    "decode fprintf present");
              check(strstr(buf, "S>=8 && (i%4==0 || i==layer_end-1)")!=NULL,
                    "prefill S>=8 cadence present");
              check(strstr(buf, "S>=1 && (i%4==0 || i==layer_end-1)")!=NULL,
                    "decode S>=1 cadence present");
              free(buf);
          }
          fclose(f);
      }
    }

    if(failures){ fprintf(stderr, "test_layer_progress: %d failure(s)\n", failures); return 1; }
    printf("test_layer_progress: ok\n");
    return 0;
}
