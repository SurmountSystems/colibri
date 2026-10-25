/* glm53_chain.h -- GLM-5.3 Flash's layers as a dense chain on the Vulkan device
 * (vk_chain.h). Included once by glm53.c in a COLI_VULKAN build, after run_layers, the
 * CPU forward it stands in for; COLI_VK_CHAIN decides (coli_vk_chain_decide, this
 * engine's integrated GPU default COLI_VK_CHAIN_UNMEASURED: off, not measured on a
 * GLM-5.3 checkpoint).
 *
 * The residual is hc_mult streams per position (mHC). What runs where, per layer, for
 * one block of rows (decode: one row):
 *   device, frame A1: the previous layer's FFN branch joins the streams (the routed sum
 *     from the host plus the shared expert, then mHC's write back); the attention site:
 *     mHC's mix (hc_fn, the split with Sinkhorn, the collapse), the input RMSNorm, then
 *     the KDA layer (the q/k/v, decay, beta and gate projections, the short convolution
 *     with its window, the delta rule with its state, the output norm and gate, o) or the
 *     MLA layer (vkc_mla_qkv, the k-pooled indexer: index keys with their LayerNorm, the
 *     pool gates, each completed pool's key, every row's pools; vkc_mla_attn over the
 *     selection), mHC's write back; the FFN site's mix, collapse and post-attention
 *     RMSNorm, and on a dense layer its MLP (clamped SwiGLU) and write back with no host
 *     step. Then the frame is waited for.
 *   host: the router and the routed experts (ffn_layer without the shared expert: the
 *     expert tier's batch and the CPU's share), the new MLA rows into the host's cache.
 *   device, frame A2 (not waited for): the shared expert (clamped SwiGLU).
 * The final streams come back to the host, which collapses them and runs the final norm
 * and the head as before.
 *
 * State and who owns it (one session at a time, `owner`):
 *   - the MLA caches (latent, index keys, pool gates): the host's stay canonical (every
 *     step copies its new rows back); the device mirrors them behind a watermark that
 *     every host write lowers (a CPU forward of the session, a pin restore rewinding
 *     `filled`); the pools' keys have a watermark of their own.
 *   - the KDA state and convolution windows: on the device while the chain runs (too
 *     large to copy per token). `where` says which side holds the newest copy; the host's
 *     is brought back before anything reads it there (a pin, a state capture, a CPU
 *     forward of the session, another session taking the device) and pushed up after
 *     anything writes it there (a restored pin or state).
 * Past the device's budget (vk_kvsplit.h) each MLA layer keeps only some blocks of its
 * latent rows on the device: the window of the newest, and the blocks the k-pooled
 * selection reads most. The attention core runs over those on the device and over the
 * rest on the CPU (from the host's canonical cache) at once, merged through the softmax
 * statistics; the index keys, pool gates and pooled keys, which every row scores, stay
 * whole.
 * A device lost while the device holds the newest KDA state: the state is rebuilt on the
 * CPU from the input rows of the positions since the host's copy was last current (the
 * chain keeps them: the embedding rows, or the vision tower's), the forward runs again on
 * the CPU and the CPU runs from there. COLI_VK_CHAIN_FAULT=n fakes the loss at the n-th
 * frame.
 *
 * The chain declines (the CPU path runs, the state synced first) for a layer range (a
 * segment), a session above the device's limits, and matrices or a geometry its shaders
 * do not take (kv_lora above 1024, qk_rope above 0 is not GLM-5.3's and is declined too,
 * a KDA head above 256 key or 128 value floats, an indexer above 64 heads, 4096 query
 * floats or k-pooling below 2).
 *
 * A partial chain (vk_chain.h, vkc_fit; docs/vulkan.md, "A partial chain"): when the
 * device's free memory does not hold every layer, the chain takes layers 0..N-1 and the
 * CPU runs N..L-1, the final collapse, norm and head (the head stays on the host unless
 * the whole chain and the head fit). N is decided at load, once the device is open and
 * before the expert cache and the tier size themselves (g53c_start, from
 * model_load_range); with COLI_VK_DENSE_HOST only the N layers give their host copies
 * back, each once all of it reached the device. A forward runs every chunk of its rows
 * through the N device layers, then run_layers goes on from layer N with the hc_mult
 * streams (H x D floats a row, once per chunk). The KDA state and windows on the device,
 * the MLA mirror, its watermarks and the KV split cover the N layers; the CPU layers'
 * state is the host's alone, so a lost device rebuilds the device layers' KDA state
 * only.
 *
 * Layers on two devices (docs/vulkan.md, "Layers on two devices"): with COLI_VK_DEV2 a
 * second chain takes the layers after the primary's on that device (its own fit from
 * layer N, COLI_VK_CHAIN_LAYERS2); a forward runs the streams through the primary's
 * layers, then through these, and the CPU runs what is left. The head stays on the host.
 * Both chains' KDA state moves together (the host's copy current for both, or for
 * neither): the second device's is read back first, so a read that fails leaves the
 * primary's untouched; the primary's record of input rows rebuilds both. Losing either
 * device turns both off. */
#include "vk_chain.h"
#include "vk_kvsplit.h"

#define G53C_HOST 0
#define G53C_DEV  1
#define G53C_BOTH 2

typedef struct {
    int ok, failed, rows, cap, pcap;
    int lo, d;                                /* its layers start at lo, on device d (vkc_device) */
    int n;                                    /* layers on the device: lo..lo+n-1 (the rest run elsewhere) */
    const GSession *owner;
    int where;                                /* G53C_HOST / DEV / BOTH for owner's KDA state */
    VkcBuf *prm;
    size_t *o_in, *o_post, *o_hca, *o_hcf, *o_qn, *o_kn, *o_ikw, *o_ikb, *o_ape, *o_conv, *o_kda;
    ColiVkTensor **fna, **fnf;                /* the mHC mix matrices, f32 */
    VkcMla *mla;
    VkcMlaCache *kv; VkcBuf **ik, **ig, **pk, **win, **st;
    int *kv_valid, *pool_valid;
    int *mla_ord, n_mla, dev_rows;            /* the MLA layers' order; device latent rows a layer */
    VkcKvSplit ks;                            /* the split past the device's budget (ks.on) */
    VkcMlaCache kvtmp;                        /* the split: a step's new rows before their slots */
    VkcMlaScratch sc;
    VkcBuf *xs, *xn, *col, *nrm, *br, *mix, *hp, *gs, *us, *hs, *ds, *qkv3, *cm, *kf, *kb, *kg, *klow, *ky;
    VkcBuf *ikd, *iq, *ihw, *igd, *isc, *sel;
    VkcBuf *h2d, *kvd, *xd, *routed;
    size_t kvd_layer;
    float *host_routed;
    /* the input rows since the host's KDA state was last current: [rec_base, rec_base + rec_len)
     * (the primary's: they rebuild both chains' layers) */
    float *rec; int rec_base, rec_len, rec_cap, rec_ok;
    unsigned long long forwards;
    double host_ms;
} G53Chain;

static G53Chain *g_g53c;
static int g_vk_chain = 0;
static int g_g53c_inited = 0;
static VkcFit g_g53c_fit;                     /* how many layers the device holds (vkc_fit), when g_g53c_fitted */
static int g_g53c_fitted;
static size_t g_g53c_lazy;                    /* what the chain allocates at its first forward: one chunk's
                                               * scratch and the N layers' MLA caches at their first size */
static size_t *g_g53c_mir;                    /* each layer's MLA caches at their first size */
/* the second device's chain (layers g_g53c_fit.n..), its fit; g_g53c_dev: the device
 * g53c_tensor uploads a new matrix to */
static G53Chain *g_g53c2;
static VkcFit g_g53c_fit2;
static int g_g53c_fit2_on, g_g53c_dev;
static G53Chain *g53c_of(int d) { return d ? g_g53c2 : g_g53c; }
static const char *g53c_name(const G53Chain *ch) { return ch && ch->d ? "glm53 dev2" : "glm53"; }
static int g53c_dev2_wanted(void) { const char *e = getenv("COLI_VK_CHAIN_DEV2"); return !(e && *e == '0'); }

/* A Mat's device copy (the per-matrix path's own, where it made one); f32 as fmt 10. The
 * second device's layers hold theirs there (g_g53c_dev at setup). */
static ColiVkTensor *g53c_tensor(const Mat *wc) {
    Mat *w = (Mat *)wc;
    if (w->vk) return (ColiVkTensor *)w->vk;
    int fmt; const void *p;
    switch (w->fmt) {
    case 0: fmt = 10; p = w->f; break;
    case 1: fmt = 1; p = w->q8; break;
    case 4: if (w->gs < 8 || w->gs % 8) return NULL; fmt = 4; p = w->q4; break;
    default: return NULL;
    }
    if (!p || w->rows < 1 || w->columns < 1) return NULL;
    const float *sc = w->fmt ? w->s : NULL;
    int gs = w->fmt == 4 ? w->gs : 0;
    return (g_g53c_dev ? coli_vk_tensor_ensure2((ColiVkTensor **)&w->vk, p, sc, fmt, w->columns, w->rows, gs)
                       : coli_vk_tensor_ensure((ColiVkTensor **)&w->vk, p, sc, fmt, w->columns, w->rows, gs)) ? (ColiVkTensor *)w->vk : NULL;
}

/* ---- the KDA state between the host and the device --------------------------------- */
/* (each covers both chains: the second device's first, see the top) */
static void g53c_rec_reset(G53Chain *ch, int base) { ch->rec_base = base; ch->rec_len = 0; ch->rec_ok = 1; }
/* One chain's copy of s's KDA state read back into the host's (DEV: BOTH); 0 = the
 * device was lost on the way (where stays DEV). */
static int g53c_sync_one(const GModel *m, const GSession *s, G53Chain *ch) {
    if (!ch || !ch->ok || ch->owner != s || ch->where != G53C_DEV || !s) return 1;
    const Cfg *c = &m->c;
    size_t ns = (size_t)c->kda_heads * c->kda_hd * c->kda_hd, nw = (size_t)3 * c->kda_proj * c->conv_k;
    int was = vkc_device(ch->d), ok = 1;
    for (int i = ch->lo; i < ch->lo + ch->n && ok; i++) {   /* the device's layers: a CPU layer's state is the host's already */
        if (c->is_full[i]) continue;
        ok = vkc_read(ch->st[i], 0, s->layer[i].kda_state, ns * sizeof(float)) &&
             vkc_read(ch->win[i], 0, s->layer[i].kda_window, nw * sizeof(float));
    }
    vkc_device(was);
    if (ok) ch->where = G53C_BOTH;
    return ok;
}
/* The host's copy of s's KDA state made current (before anything reads it there). */
static void g53c_sync_host(const GModel *m, const GSession *s) {
    G53Chain *ch = g_g53c, *ch2 = g_g53c2;
    int dev = (ch && ch->ok && ch->owner == s && ch->where == G53C_DEV) ||
              (ch2 && ch2->ok && ch2->owner == s && ch2->where == G53C_DEV);
    if (!dev || !s) return;
    if (!g53c_sync_one(m, s, ch2) || !g53c_sync_one(m, s, ch)) {
        fprintf(stderr, "[VK] glm53 chain: the device was lost holding the KDA state; it is rebuilt at the next step\n");
        return;   /* where stays DEV: the next forward rebuilds from the record */
    }
    if (ch && ch->ok && ch->owner == s) g53c_rec_reset(ch, s->filled);
}
/* The host wrote s's KDA state (a restored pin or state). */
static void g53c_host_wrote(const GSession *s) {
    for (int d = 0; d < 2; d++) { G53Chain *ch = g53c_of(d); if (ch && ch->owner == s) ch->where = G53C_HOST; }
}
/* s goes away: whatever the devices hold for it goes with it. */
static void g53c_session_gone(const GSession *s) {
    for (int d = 0; d < 2; d++) {
        G53Chain *ch = g53c_of(d);
        if (ch && ch->owner == s) { ch->owner = NULL; ch->where = G53C_HOST; }
    }
}
/* The CPU runs one chain's layers of s from `start`: the device's state stale after (and
 * the MLA rows from start on). */
static void g53c_cpu_one(const GModel *m, G53Chain *ch, int start) {
    ch->where = G53C_HOST;
    for (int i = ch->lo; i < ch->lo + ch->n; i++) {
        if (ch->kv_valid[i] > start) ch->kv_valid[i] = start;
        if (ch->pool_valid[i] > start / m->c.index_kpool) ch->pool_valid[i] = start / m->c.index_kpool;
        if (m->c.is_full[i]) vkc_kv_lower(&ch->ks, ch->mla_ord[i], start);
    }
}
/* A CPU forward of s from `start`: its state current on the host first. */
static void g53c_cpu_step(const GModel *m, const GSession *s, int start) {
    G53Chain *ch = g_g53c;
    if (!ch || !ch->ok || ch->owner != s) return;
    g53c_sync_host(m, s);
    for (int d = 0; d < 2; d++) { G53Chain *x = g53c_of(d); if (x && x->ok && x->owner == s) g53c_cpu_one(m, x, start); }
}

/* ---- setup ----------------------------------------------------------------------------- */
/* g53c_tensor's backend format for w without uploading it (-1: no device form) and its
 * group size; the fit counts the bytes from it. */
static int g53c_form(const Mat *w, int *gs) {
    *gs = 0;
    if (w->rows < 1 || w->columns < 1) return -1;
    switch (w->fmt) {
    case 0: return w->f ? 10 : -1;
    case 1: return w->q8 ? 1 : -1;
    case 4: if (w->gs < 8 || w->gs % 8 || !w->q4) return -1; *gs = w->gs; return 4;
    default: return -1;
    }
}
/* Layer i's matrices on the device (the chain's set; up to 13) */
static int g53c_layer_mats(const GModel *m, int i, Mat **w) {
    const Cfg *c = &m->c; GLayer *l = &m->layer[i]; int n = 0;
    if (c->is_full[i]) {
        Mat *a[] = {&l->qa, &l->qb, &l->kva, &l->kvb_kt, &l->kvb_v, &l->o, &l->iwq, &l->iwk, &l->iwp, &l->ikpg};
        for (size_t k = 0; k < sizeof a / sizeof *a; k++) w[n++] = a[k];
    } else {
        Mat *a[] = {&l->kq, &l->kk, &l->kv, &l->ko, &l->kga, &l->kgb, &l->kfa, &l->kfb, &l->kb};
        for (size_t k = 0; k < sizeof a / sizeof *a; k++) w[n++] = a[k];
    }
    if (i < c->first_dense) { w[n++] = &l->dg; w[n++] = &l->du; w[n++] = &l->dd; }
    else { w[n++] = &l->rg; w[n++] = &l->ru; w[n++] = &l->rd; }
    return n;
}
/* What the chain's shaders do not take (NULL: nothing), from the shapes alone: no upload. */
static const char *g53c_check(const GModel *m) {
    const Cfg *c = &m->c; int L = c->n_layers, H = c->hc_mult, P = c->kda_proj;
    if (m->layer_begin != 0 || m->layer_end != L || !m->has_io) return "a layer range";
    if (c->kv_lora > 1024 || c->qk_rope != 0 || c->qk_nope > 1024 || c->q_lora < 1) return "an attention geometry its shaders do not take";
    if (c->index_nh > 64 || c->index_nh * c->index_hd > 4096 || c->index_kpool < 2 || c->index_topk % c->index_kpool ||
        c->index_topk / c->index_kpool > 1024) return "an indexer its shaders do not take";
    if (P > 0 && (c->kda_hd > 128 || c->conv_k > 8)) return "a KDA head its shaders do not take";
    if (H < 1 || H > 8 || c->hc_iters < 1) return "hyper-connections its shaders do not take";
    Mat *w[16]; int gs;
    for (int i = 0; i < L; i++) {
        int n = g53c_layer_mats(m, i, w);
        for (int k = 0; k < n; k++)
            if (g53c_form(w[k], &gs) < 0) return "a matrix with no device form";
    }
    return NULL;
}
/* Layer i's share of the parameter arena, in floats */
static size_t g53c_arena_floats(const Cfg *c, int i) {
    size_t nm = (size_t)(2 + c->hc_mult) * c->hc_mult, n = 2 * (size_t)c->hidden + 2 * (3 + nm);
    if (c->is_full[i]) n += (size_t)c->q_lora + c->kv_lora + 2 * (size_t)c->index_hd + (size_t)c->index_kpool * c->index_hd;
    else n += (size_t)3 * c->kda_proj * c->conv_k + (size_t)c->kda_heads + c->kda_proj + c->kda_hd;
    return n;
}

/* g53c_res counts instead of reserving while g_g53c_count >= 0 (the chunk's sizing); with
 * g_g53c_count_fit, each buffer at the bytes the device gives it (the fit's scratch) */
static long long g_g53c_count = -1;
static int g_g53c_count_fit;
static int g53c_res(VkcBuf **b, size_t floats, int kind) {
    size_t bytes = (floats ? floats : 1) * sizeof(float);
    if (g_g53c_count >= 0) { g_g53c_count += (long long)(g_g53c_count_fit ? vkc_fit_buf(bytes) : bytes); return 1; }
    return vkc_reserve(b, bytes, kind);
}
static int g53c_scratch(G53Chain *ch, const GModel *m, int rows, int ctx);

/* The fit (vkc_fit), before any upload: each layer's device bytes (the mHC mixes and its
 * matrices as g53c_setup uploads them, its share of the parameter arena, a KDA layer's
 * state and window, an MLA layer's caches at the first size g53c_mirror gives them), the
 * chain's fixed bytes (the scratch of one prompt chunk of vkc_fit_rows(128) rows at the
 * context GLM53_MAXT, the arena's alignment) and the tail: the head, which goes up only
 * with every layer there (COLI_VK_DENSE_HOST and the per-matrix path place it). */
static void g53c_fit_plan(GModel *m) {
    const Cfg *c = &m->c;
    int L = c->n_layers, D = c->hidden, H = c->hc_mult, HD = H * D, nm = (2 + H) * H, P = c->kda_proj, ID = c->index_hd;
    size_t *lb = calloc(L ? L : 1, sizeof(size_t)), *mb = calloc(L ? L : 1, sizeof(size_t));
    size_t *mir = calloc(L ? L : 1, sizeof(size_t));
    if (!lb || !mb || !mir) { free(lb); free(mb); free(mir); return; }
    const int cap0 = 256;   /* g53c_mirror's first size */
    for (int i = 0; i < L; i++) {
        lb[i] = 2 * vkc_fit_tensor(10, HD, nm, 0);
        mb[i] = 2 * coli_vk_tensor_payload(10, HD, nm, 0);
        Mat *w[16]; int gs, n = g53c_layer_mats(m, i, w);
        for (int k = 0; k < n; k++) {
            int f = g53c_form(w[k], &gs);
            lb[i] += vkc_fit_tensor(f, w[k]->columns, w[k]->rows, gs);
            mb[i] += coli_vk_tensor_payload(f, w[k]->columns, w[k]->rows, gs);
        }
        lb[i] += g53c_arena_floats(c, i) * sizeof(float);
        if (c->is_full[i])
            mir[i] = vkc_fit_buf((size_t)cap0 * c->kv_lora * sizeof(float)) + 2 * vkc_fit_buf((size_t)cap0 * ID * sizeof(float)) +
                     vkc_fit_buf((size_t)(cap0 / c->index_kpool + 1) * ID * sizeof(float));
        else
            lb[i] += vkc_fit_buf((size_t)3 * P * c->conv_k * sizeof(float)) +
                     vkc_fit_buf((size_t)c->kda_heads * c->kda_hd * c->kda_hd * sizeof(float));
        lb[i] += mir[i];
    }
    /* one chunk's scratch: g53c_scratch's buffers (every layer's rows down) and the MLA
     * scratch at the context of a serve slot */
    int R = vkc_fit_rows(128), ctx = getenv("GLM53_MAXT") ? atoi(getenv("GLM53_MAXT")) : 8192;
    if (ctx < 64) ctx = 64;
    G53Chain tmp; memset(&tmp, 0, sizeof tmp); tmp.n = L;
    g_g53c_count = 0; g_g53c_count_fit = 1; g53c_scratch(&tmp, m, R, ctx);
    size_t scratch = (size_t)g_g53c_count;
    g_g53c_count = -1; g_g53c_count_fit = 0;
    int any_mla = 0; for (int i = 0; i < L; i++) any_mla |= c->is_full[i];
    if (any_mla) {
        size_t r = (size_t)R, Hh = (size_t)c->n_heads;
        scratch += vkc_fit_buf(r * c->q_lora * 4) + vkc_fit_buf(r * Hh * c->qk_nope * 4) + vkc_fit_buf(r * c->kv_lora * 4) +
                   2 * vkc_fit_buf(r * Hh * c->kv_lora * 4) + vkc_fit_buf(r * Hh * c->v_head * 4);
    }
    size_t fixed = scratch + vkc_fit_buf(4), tail = 0;   /* + the arena's alignment */
    { int gs, f = m->head.resident ? g53c_form(&m->head, &gs) : -1;
      if (f >= 0) tail = vkc_fit_tensor(f, m->head.columns, m->head.rows, gs); }
    vkc_fit("glm53", L, lb, mb, fixed, tail, &g_g53c_fit);
    g_g53c_fitted = 1;
    g_g53c_lazy = scratch;
    g_g53c_mir = mir;
    /* the layers the primary leaves, on COLI_VK_DEV2's device: a fit of its own from layer
     * n0 (the head stays on the host), with that device's free memory, its pipelines up now */
    int n0 = g_g53c_fit.n;
    if (vkc_fit_partial(&g_g53c_fit) && n0 > 0 && n0 < L && g53c_dev2_wanted() && getenv("COLI_VK_DEV2") &&
        coli_vk_dev2_open_env()) {
        vkc_device(1);
        int n2 = vkc_fit("glm53 dev2", L - n0, lb + n0, mb + n0, fixed, 0, &g_g53c_fit2);
        if (n2 > 0 && !(vkc_init() && vkc_mla_ready() && vkc_kda_ready() && vkc_mhc_ready())) {
            fprintf(stderr, "[VK] glm53 chain: the second device's pipelines did not come up; its layers stay on the CPU\n");
            n2 = 0;
        }
        g_g53c_fit2_on = n2 > 0;
        vkc_device(0);
    }
    free(lb); free(mb);
}

/* Layer i's device objects (its mHC mixes, matrices, KDA state and window) freed: a layer
 * that did not fully reach the device leaves nothing there. */
static void g53c_layer_free(G53Chain *ch, GModel *m, int i) {
    Mat *w[16]; int n = g53c_layer_mats(m, i, w);
    ColiVkTensor **t[18]; int nt = 0;
    t[nt++] = &ch->fna[i]; t[nt++] = &ch->fnf[i];
    for (int k = 0; k < n; k++) t[nt++] = (ColiVkTensor **)&w[k]->vk;
    VkcBuf **b[2] = {&ch->win[i], &ch->st[i]};
    vkc_layer_free(t, nt, b, 2);
}

/* The parameters and layers 0..N-1 on the device (N = the fit's), layer by layer: its mHC
 * mixes, its matrices and a KDA layer's state and window, then (COLI_VK_DENSE_HOST) its
 * host copies given back once all of it is there. A layer that does not reach the device
 * is freed whole and the chain keeps the layers before it (vkc_fit_shrink). 0 = no layer
 * on the device: the chain stays off. d = 1: the second device's chain, its layers from
 * g_g53c_fit.n with copies of their own there (device 1 current; the host keeps its). */
static int g53c_setup_dev(GModel *m, int d) {
    const Cfg *c = &m->c;
    int L = c->n_layers, D = c->hidden, H = c->hc_mult, HD = H * D, nm = (2 + H) * H, P = c->kda_proj;
    VkcFit *fit = d ? &g_g53c_fit2 : &g_g53c_fit;
    int N = fit->n, lo = d ? g_g53c_fit.n : 0;
    const char *nmc = d ? "glm53 dev2" : "glm53";
    G53Chain *ch = calloc(1, sizeof *ch);
    if (!ch) return 0;
    ch->d = d; ch->lo = lo;
    size_t **offs[] = {&ch->o_in, &ch->o_post, &ch->o_hca, &ch->o_hcf, &ch->o_qn, &ch->o_kn, &ch->o_ikw, &ch->o_ikb,
                       &ch->o_ape, &ch->o_conv, &ch->o_kda};
    for (size_t k = 0; k < sizeof offs / sizeof *offs; k++) if (!(*offs[k] = calloc(L, sizeof(size_t)))) return 0;
    ch->fna = calloc(L, sizeof(void *)); ch->fnf = calloc(L, sizeof(void *));
    ch->mla = calloc(L, sizeof(VkcMla)); ch->kv = calloc(L, sizeof(VkcMlaCache));
    ch->ik = calloc(L, sizeof(void *)); ch->ig = calloc(L, sizeof(void *)); ch->pk = calloc(L, sizeof(void *));
    ch->win = calloc(L, sizeof(void *)); ch->st = calloc(L, sizeof(void *));
    ch->kv_valid = calloc(L, sizeof(int)); ch->pool_valid = calloc(L, sizeof(int)); ch->mla_ord = calloc(L, sizeof(int));
    if (!ch->fna || !ch->fnf || !ch->mla || !ch->kv || !ch->ik || !ch->ig || !ch->pk || !ch->win || !ch->st ||
        !ch->kv_valid || !ch->pool_valid || !ch->mla_ord) return 0;
    if (d) g_g53c2 = ch; else g_g53c = ch;
    /* the parameter arena of the N layers */
    size_t n = 0;
    for (int i = lo; i < lo + N; i++) {
        ch->o_in[i] = n; n += D; ch->o_post[i] = n; n += D;
        ch->o_hca[i] = n; n += 3 + nm; ch->o_hcf[i] = n; n += 3 + nm;
        if (c->is_full[i]) {
            ch->o_qn[i] = n; n += c->q_lora; ch->o_kn[i] = n; n += c->kv_lora;
            ch->o_ikw[i] = n; n += c->index_hd; ch->o_ikb[i] = n; n += c->index_hd;
            ch->o_ape[i] = n; n += (size_t)c->index_kpool * c->index_hd;
        } else {
            ch->o_conv[i] = n; n += (size_t)3 * P * c->conv_k;
            ch->o_kda[i] = n; n += (size_t)c->kda_heads + P + c->kda_hd;
        }
    }
    float *a = calloc(n ? n : 1, sizeof(float));
    if (!a) return 0;
    for (int i = lo; i < lo + N; i++) {
        const GLayer *l = &m->layer[i];
        memcpy(a + ch->o_in[i], l->in_ln, D * sizeof(float));
        memcpy(a + ch->o_post[i], l->post_ln, D * sizeof(float));
        memcpy(a + ch->o_hca[i], l->hc_attn_scale, 3 * sizeof(float));
        memcpy(a + ch->o_hca[i] + 3, l->hc_attn_base, nm * sizeof(float));
        memcpy(a + ch->o_hcf[i], l->hc_ffn_scale, 3 * sizeof(float));
        memcpy(a + ch->o_hcf[i] + 3, l->hc_ffn_base, nm * sizeof(float));
        if (c->is_full[i]) {
            memcpy(a + ch->o_qn[i], l->qa_ln, c->q_lora * sizeof(float));
            memcpy(a + ch->o_kn[i], l->kva_ln, c->kv_lora * sizeof(float));
            memcpy(a + ch->o_ikw[i], l->ik_nw, c->index_hd * sizeof(float));
            if (l->ik_nb) memcpy(a + ch->o_ikb[i], l->ik_nb, c->index_hd * sizeof(float));
            memcpy(a + ch->o_ape[i], l->ikpa, (size_t)c->index_kpool * c->index_hd * sizeof(float));
        } else {
            memcpy(a + ch->o_conv[i], l->conv, (size_t)3 * P * c->conv_k * sizeof(float));
            memcpy(a + ch->o_kda[i], l->alog, c->kda_heads * sizeof(float));
            memcpy(a + ch->o_kda[i] + c->kda_heads, l->dt, P * sizeof(float));
            memcpy(a + ch->o_kda[i] + c->kda_heads + P, l->onorm, c->kda_hd * sizeof(float));
        }
    }
    ch->prm = vkc_buf(n * sizeof(float), VKC_DEV);
    int ok = ch->prm && vkc_begin() && vkc_write(ch->prm, 0, a, n * sizeof(float)) && vkc_submit(1);
    free(a);
    if (!ok) {
        vkc_free(ch->prm); ch->prm = NULL;
        vkc_fit_shrink(nmc, fit, 0, vkc_lost() ? "the device was lost" : "the parameters' buffer was refused");
        return 0;
    }
    g_g53c_dev = d;
    for (int i = lo; i < lo + N; i++) {
        GLayer *l = &m->layer[i];
        Mat *w[16]; int nw = g53c_layer_mats(m, i, w);
        int up = d ? coli_vk_tensor_ensure2(&ch->fna[i], l->hc_attn_fn, NULL, 10, HD, nm, 0) &&
                     coli_vk_tensor_ensure2(&ch->fnf[i], l->hc_ffn_fn, NULL, 10, HD, nm, 0)
                   : coli_vk_tensor_ensure(&ch->fna[i], l->hc_attn_fn, NULL, 10, HD, nm, 0) &&
                     coli_vk_tensor_ensure(&ch->fnf[i], l->hc_ffn_fn, NULL, 10, HD, nm, 0);
        for (int k = 0; k < nw && up; k++) up = g53c_tensor(w[k]) != NULL;
        if (up && !c->is_full[i]) {
            ch->win[i] = vkc_buf((size_t)3 * P * c->conv_k * sizeof(float), VKC_DEV);
            ch->st[i] = vkc_buf((size_t)c->kda_heads * c->kda_hd * c->kda_hd * sizeof(float), VKC_DEV);
            up = ch->win[i] && ch->st[i];
        }
        if (!up) {   /* free what it placed: the layer runs on the CPU, from its host copies */
            g53c_layer_free(ch, m, i);
            vkc_fit_shrink(nmc, fit, i - lo, vkc_lost() ? "the device was lost" : "a matrix or buffer was refused");
            break;
        }
        if (c->is_full[i])
            ch->mla[i] = (VkcMla){c->n_heads, c->qk_nope, 0, c->v_head, c->kv_lora, D, c->q_lora, c->eps,
                                  1.0f / sqrtf((float)c->qk_nope), VKC_ROPE_HALF, l->qa.vk, l->qb.vk, l->kva.vk, NULL,
                                  l->kvb_kt.vk, l->kvb_v.vk, l->o.vk, ch->prm, ch->o_qn[i], ch->o_kn[i]};
        if (!d) g53_dho_drop_layer(m, i);   /* COLI_VK_DENSE_HOST: all of the layer is there, its host copies go */
        vkc_fit_mark(fit, i - lo);
    }
    g_g53c_dev = 0;
    ch->n = fit->n;
    if (!ch->n) { vkc_free(ch->prm); ch->prm = NULL; return 0; }
    for (int i = lo; i < lo + ch->n; i++) if (c->is_full[i]) ch->mla_ord[i] = ch->n_mla++;
    ch->ok = 1;
    int full = 0, dense = 0;
    for (int i = lo; i < lo + ch->n; i++) { full += c->is_full[i]; dense += i < c->first_dense; }
    if (d) fprintf(stderr, "[VK] glm53 chain: layers %d..%d on the second device (%d KDA, %d MLA with the k-pooled indexer, "
                           "%d dense), %d streams, %.1f MiB of parameters\n", lo, lo + ch->n - 1, ch->n - full, full, dense, H,
                   n * 4 / 1048576.0);
    else fprintf(stderr, "[VK] glm53 chain: %d layers on the device (%d KDA, %d MLA with the k-pooled indexer, %d dense), "
                         "%d streams, %.1f MiB of parameters\n", ch->n, ch->n - full, full, dense, H, n * 4 / 1048576.0);
    return 1;
}
static int g53c_setup(GModel *m) { return g53c_setup_dev(m, 0); }

/* Prompt rows per chunk (vkc_chunk_rows): the chain's scratch a row, counted from the
 * reservations (the MLA scratch as vkc_mla_scratch sizes it, the indexer's scores at
 * the session's capacity), and the routed experts' outputs (the tier's rows, the
 * host's sum) for it. */
static int g53c_rows(G53Chain *ch, const GModel *m, int ctx) {
    if (!vkc_chunk_auto()) return vkc_chunk_rows(g53c_name(ch), 0);
    const Cfg *c = &m->c;
    size_t mla = 0;
    for (int i = ch->lo; i < ch->lo + ch->n; i++) if (c->is_full[i]) {
        const VkcMla *a = &ch->mla[i];
        mla = (size_t)(a->q_lora > 0 ? a->q_lora : 1) + (size_t)a->H * (a->Q + a->R) + (size_t)(a->K + a->R) +
              2 * (size_t)a->H * a->K + (size_t)a->H * a->V;
        break;
    }
    size_t ksz = ch->kvd_layer;
    g_g53c_count = 0; g53c_scratch(ch, m, 1, ctx); long long b1 = g_g53c_count;
    g_g53c_count = 0; g53c_scratch(ch, m, 2, ctx); long long b2 = g_g53c_count;
    g_g53c_count = -1; ch->kvd_layer = ksz;
    size_t row = (size_t)(b2 - b1) + mla * sizeof(float) + (size_t)(2 * c->topk + 2) * c->hidden * sizeof(float);
    /* ffn_layer_ex allocates both activation matrices and its gather/output
     * buffers even when the tier takes every routed assignment. */
    size_t wide = c->dense_inter > c->moe_inter ? c->dense_inter : c->moe_inter;
    row += 2 * (wide + c->hidden) * sizeof(float);
    return vkc_chunk_rows(g53c_name(ch), row);
}

static int g53c_scratch(G53Chain *ch, const GModel *m, int rows, int ctx) {
    const Cfg *c = &m->c;
    int D = c->hidden, H = c->hc_mult, HD = H * D, nm = (2 + H) * H, P = c->kda_proj;
    int IH = c->index_nh, ID = c->index_hd, MI = c->dense_inter > c->moe_inter ? c->dense_inter : c->moe_inter;
    int width = coli_sparse_index_width(c->index_topk, c->index_kpool, c->index_kpool_tail);
    int mfull = -1; for (int i = ch->lo; i < ch->lo + ch->n; i++) if (c->is_full[i]) { mfull = i; break; }   /* the device's MLA layers */
    size_t r = (size_t)rows;
    ch->kvd_layer = r * (c->kv_lora + 2 * ID);
    int ok = (mfull < 0 || g_g53c_count >= 0 || vkc_mla_scratch(&ch->sc, &ch->mla[mfull], rows)) &&
             g53c_res(&ch->xs, r * HD, VKC_DEV) && g53c_res(&ch->xn, r * HD, VKC_DEV) && g53c_res(&ch->col, r * D, VKC_DEV) &&
             g53c_res(&ch->nrm, r * D, VKC_DEV) && g53c_res(&ch->br, r * D, VKC_DEV) && g53c_res(&ch->mix, r * nm, VKC_DEV) &&
             g53c_res(&ch->hp, r * (2 * H + H * H), VKC_DEV) && g53c_res(&ch->gs, r * MI, VKC_DEV) &&
             g53c_res(&ch->us, r * MI, VKC_DEV) && g53c_res(&ch->hs, r * MI, VKC_DEV) && g53c_res(&ch->ds, r * D, VKC_DEV) &&
             g53c_res(&ch->h2d, r * D, VKC_DOWN) && g53c_res(&ch->kvd, (size_t)ch->n * ch->kvd_layer, VKC_DOWN) &&
             g53c_res(&ch->xd, r * HD, VKC_DOWN) && g53c_res(&ch->routed, r * D, VKC_UP);
    if (ok && P > 0)
        ok = g53c_res(&ch->qkv3, 3 * r * P, VKC_DEV) && g53c_res(&ch->cm, r * 3 * P, VKC_DEV) && g53c_res(&ch->kf, r * P, VKC_DEV) &&
             g53c_res(&ch->kb, r * c->kda_heads, VKC_DEV) && g53c_res(&ch->kg, r * P, VKC_DEV) &&
             g53c_res(&ch->klow, r * c->kda_hd, VKC_DEV) && g53c_res(&ch->ky, r * P, VKC_DEV);
    if (ok && mfull >= 0)
        ok = g53c_res(&ch->ikd, r * ID, VKC_DEV) && g53c_res(&ch->iq, r * IH * ID, VKC_DEV) && g53c_res(&ch->ihw, r * IH, VKC_DEV) &&
             g53c_res(&ch->igd, r * ID, VKC_DEV) && g53c_res(&ch->isc, r * (size_t)(ctx / c->index_kpool + 1), VKC_DEV) &&
             g53c_res(&ch->sel, r * (size_t)(1 + width), VKC_DEV);
    if (g_g53c_count >= 0) {   /* count the split scratch without allocating or changing capacities */
        if (ch->ks.on && mfull >= 0)
            g_g53c_count += (long long)r * (3LL * c->n_heads * (c->kv_lora + 2) +
                                  (long long)c->n_heads * c->kv_lora + c->kv_lora + 1 +
                                  coli_sparse_index_width(c->index_topk, c->index_kpool, c->index_kpool_tail)) * sizeof(float);
        return ok;
    }
    if (ok && ch->ks.on && mfull >= 0)   /* the split: the partial results, a step's new rows before their slots */
        ok = vkc_kv_parts(&ch->ks, r * c->n_heads * (c->kv_lora + 2)) &&
             (ch->kvtmp.cap >= rows || (g53c_res(&ch->kvtmp.lat, r * c->kv_lora, VKC_DEV) && (ch->kvtmp.cap = rows)));
    if (!ok) return 0;
    if (ch->rows < rows) {
        float *hr = realloc(ch->host_routed, r * D * sizeof(float));
        if (!hr) return 0;
        ch->host_routed = hr; ch->rows = rows;
    }
    return 1;
}

/* The MLA mirror: room for `need` positions (the owner's), filled again when it grows,
 * and planned again then (vkc_kv_plan): whole when it fits the device's budget, else the
 * split (the latent rows; the index keys, pool gates and pooled keys stay whole), for
 * steps of `rows` rows. */
static int g53c_mirror(G53Chain *ch, const GModel *m, int need, int limit, int rows) {
    const Cfg *c = &m->c; int lo = ch->lo, hi = lo + ch->n, ID = c->index_hd, pool = c->index_kpool;   /* the device's layers */
    if (ch->cap >= need) return 1;
    int cap = 256; while (cap < need) cap *= 2;
    if (cap > limit) cap = limit;
    if (cap < need) return 0;
    size_t row = (size_t)c->kv_lora * sizeof(float);
    if (ch->n_mla && !vkc_kv_plan(&ch->ks, g53c_name(ch), ch->n_mla, row, cap, rows, 1, (size_t)ch->n_mla * ch->dev_rows * row)) return 0;
    int dr = ch->ks.on ? ch->ks.rows : cap;
    for (int i = lo; i < hi; i++) {
        if (!c->is_full[i]) continue;
        vkc_free(ch->kv[i].lat); vkc_free(ch->ik[i]); vkc_free(ch->ig[i]); vkc_free(ch->pk[i]);
        ch->kv[i] = (VkcMlaCache){NULL, NULL, 0}; ch->ik[i] = ch->ig[i] = ch->pk[i] = NULL;
        ch->kv_valid[i] = ch->pool_valid[i] = 0;
    }
    ch->cap = 0; ch->dev_rows = 0;
    for (int i = lo; i < hi; i++) {
        if (!c->is_full[i]) continue;
        ch->kv[i].lat = vkc_buf((size_t)dr * c->kv_lora * sizeof(float), VKC_DEV); ch->kv[i].cap = dr;
        ch->ik[i] = vkc_buf((size_t)cap * ID * sizeof(float), VKC_DEV);
        ch->ig[i] = vkc_buf((size_t)cap * ID * sizeof(float), VKC_DEV);
        ch->pk[i] = vkc_buf((size_t)(cap / pool + 1) * ID * sizeof(float), VKC_DEV);
        if (!ch->kv[i].lat || !ch->ik[i] || !ch->ig[i] || !ch->pk[i]) return 0;
    }
    ch->cap = cap; ch->dev_rows = dr;
    return 1;
}

/* Record the uploads that make the device the owner's below `start`, for a step of
 * n_rows rows (the split: its window placed first). */
static int g53c_push(G53Chain *ch, const GModel *m, const GSession *s, int start, int n_rows) {
    const Cfg *c = &m->c; int ok = 1, K = c->kv_lora, ID = c->index_hd;
    if (ch->where == G53C_HOST) {
        size_t ns = (size_t)c->kda_heads * c->kda_hd * c->kda_hd, nw = (size_t)3 * c->kda_proj * c->conv_k;
        for (int i = ch->lo; i < ch->lo + ch->n && ok; i++)
            if (!c->is_full[i])
                ok = vkc_write(ch->st[i], 0, s->layer[i].kda_state, ns * sizeof(float)) &&
                     vkc_write(ch->win[i], 0, s->layer[i].kda_window, nw * sizeof(float));
        ch->where = G53C_BOTH;
    }
    for (int i = ch->lo; i < ch->lo + ch->n && ok; i++) {
        if (!c->is_full[i]) continue;
        const GLayerState *ls = &s->layer[i];
        int t0 = ch->kv_valid[i], n = start - t0;
        if (ch->pool_valid[i] > start / c->index_kpool) ch->pool_valid[i] = start / c->index_kpool;   /* rows from start change */
        if (ch->ks.on) {      /* the split: the window placed, its latent rows below start uploaded */
            VkcKvPart pl = {1, K, ls->latent, 0, ch->kv[i].lat, 0};
            vkc_kv_place(&ch->ks, ch->mla_ord[i], start, n_rows);
            ok = vkc_kv_push(&ch->ks, ch->mla_ord[i], &pl, 1, start);
        }
        if (n <= 0 || !ok) continue;
        ok = (ch->ks.on || vkc_write(ch->kv[i].lat, (size_t)t0 * K, ls->latent + (size_t)t0 * K, (size_t)n * K * sizeof(float))) &&
             vkc_write(ch->ik[i], (size_t)t0 * ID, ls->ikeys + (size_t)t0 * ID, (size_t)n * ID * sizeof(float)) &&
             vkc_write(ch->ig[i], (size_t)t0 * ID, ls->igates + (size_t)t0 * ID, (size_t)n * ID * sizeof(float));
        ch->kv_valid[i] = start;
    }
    return ok;
}
static void g53c_pull(G53Chain *ch, const GModel *m, GSession *s, int from, int to, int pb, int n) {
    const Cfg *c = &m->c; int K = c->kv_lora, ID = c->index_hd;
    for (int i = from; i < to; i++) {
        if (!c->is_full[i]) continue;
        const float *kv = (const float *)vkc_ptr(ch->kvd) + (size_t)(i - ch->lo) * ch->kvd_layer;
        GLayerState *ls = &s->layer[i];
        memcpy(ls->latent + (size_t)pb * K, kv, (size_t)n * K * sizeof(float));
        memcpy(ls->ikeys + (size_t)pb * ID, kv + (size_t)n * K, (size_t)n * ID * sizeof(float));
        memcpy(ls->igates + (size_t)pb * ID, kv + (size_t)n * (K + ID), (size_t)n * ID * sizeof(float));
    }
}

/* ---- one layer's pieces ---------------------------------------------------------------- */
static FILE *g53c_dump_fp(void);
static int g53c_dump_cpu_layers(const GModel *m, float *xs, const float *xin, int nr, int start,
                                int lo, int upto, float *nxt, float *col, float *post, float *comb,
                                float *nrm, float *br, float *sg, float *su);
static int g53c_dump_cpu_to_post(const GModel *m, const float *xin, int nr, int start,
                                 int lo, int layer, int site,
                                 float *residual, float *branch, float *post, float *comb,
                                 float *nxt, float *col, float *nrm, float *sg, float *su);
/* COLI_VK_DUMP_FMT1: original streams of the live chunk, for last-layer POST internals. */
static const float *g53c_dump_xin;
static int g53c_dump_nr, g53c_dump_start, g53c_dump_lo;
static void g53c_dump_maxdiff(FILE *df, const char *tag, const float *a, const float *b, size_t n) {
    double d = 0, m = 0;
    for (size_t k = 0; k < n; k++) {
        double x = fabs((double)a[k] - (double)b[k]); if (x > d) d = x;
        double y = fabs((double)b[k]); if (y > m) m = y;
    }
    fprintf(df, "[dump] %s: max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
            tag, d, m, m > 0 ? d / m : 0, 1e-4 * m);
}
/* POST internals of already-diverged streams: sequential RMS of H*D and unweighted /H mean. */
static void g53c_dump_stream_rms_mean(FILE *df, const char *tag, int n, int H, int D, float eps,
                                     const float *gpu, const float *cpu) {
    int HD = H * D;
    double dr = 0, mr = 0, dm = 0, mmv = 0;
    for (int t = 0; t < n; t++) {
        const float *g = gpu + (size_t)t * HD, *c0 = cpu + (size_t)t * HD;
        float gss = 0.f, css = 0.f;
        for (int i = 0; i < HD; i++) { gss += g[i] * g[i]; css += c0[i] * c0[i]; }
        float gr = 1.f / sqrtf(gss / (float)HD + eps);
        float cr = 1.f / sqrtf(css / (float)HD + eps);
        double a = fabs((double)gr - (double)cr); if (a > dr) dr = a;
        double b = fabs((double)cr); if (b > mr) mr = b;
        for (int d0 = 0; d0 < D; d0++) {
            float gs = 0.f, cs = 0.f;
            for (int h = 0; h < H; h++) { gs += g[h * D + d0]; cs += c0[h * D + d0]; }
            gs /= (float)H; cs /= (float)H;
            double am = fabs((double)gs - (double)cs); if (am > dm) dm = am;
            double bm = fabs((double)cs); if (bm > mmv) mmv = bm;
        }
    }
    fprintf(df, "[dump] %s RMS (1/sqrt(mean x^2 + eps)): S=%d HD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
            tag, n, HD, dr, mr, mr > 0 ? dr / mr : 0, 1e-4 * mr);
    fprintf(df, "[dump] %s stream mean (/H): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
            tag, n, D, dm, mmv, mmv > 0 ? dm / mmv : 0, 1e-4 * mmv);
}
/* mHC's entry for a site: the mix, the split, the collapse, the RMSNorm into nrm */
static int g53c_pre(G53Chain *ch, const GModel *m, ColiVkTensor *fn, size_t hco, size_t lno, int n) {
    const Cfg *c = &m->c; int H = c->hc_mult, D = c->hidden, HD = H * D, nm = (2 + H) * H, hr = 2 * H + H * H;
    VkcMhc sp = {n, H, D, c->hc_iters, 0, HD, 0, nm, 0, hr, 0, D, (int)hco, 0, c->eps, c->hc_eps, 0.f};
    VkcNorm nr = {n, D, 1, 0, D, D, 0, D, D, (int)lno, 0, 0, c->eps, 1.f};
    int ok = vkc_matmul(fn, ch->xs, 0, ch->mix, 0, n) && vkc_mhc(VKC_MHC_SPLIT, ch->xs, ch->mix, ch->hp, ch->prm, NULL, &sp) &&
             vkc_mhc(VKC_MHC_COLLAPSE, ch->xs, NULL, ch->hp, NULL, ch->col, &sp) && vkc_norm(ch->col, ch->prm, ch->nrm, &nr);
    static int nrm_n;
    FILE *df = g53c_dump_fp();
    /* count every PRE of a multi-row chunk so last-layer FFN nrm is nrm_n==7 (4 layers * 2 sites). */
    int dump_nrm = df && n > 1 && (nrm_n == 0 || nrm_n == 7);
    if (ok && df && n > 1) {
        if (dump_nrm) {
        const char *nrm_name = nrm_n == 0 ? "first-layer nrm (GPU vs CPU rms of GPU col)"
                                          : "last-layer nrm into FFN (GPU vs CPU rms of GPU col)";
        size_t nd = (size_t)n * (size_t)D;
        float *col = malloc(nd * sizeof(float)), *gn = malloc(nd * sizeof(float)), *cn = malloc(nd * sizeof(float));
        int rd = col && gn && cn && vkc_read(ch->col, 0, col, nd * 4) && vkc_read(ch->nrm, 0, gn, nd * 4);
        if (!rd) fprintf(df, "[dump] %s: not sampled\n", nrm_name);
        else {
            const float *w = nrm_n == 0 ? m->layer[0].in_ln : m->layer[c->n_layers - 1].post_ln;
            for (int t = 0; t < n; t++) rms(cn + (size_t)t * D, col + (size_t)t * D, w, D, c->eps);
            double d = 0, mc = 0;
            for (size_t k = 0; k < nd; k++) {
                double a = fabs((double)gn[k] - (double)cn[k]); if (a > d) d = a;
                double b = fabs((double)cn[k]); if (b > mc) mc = b;
            }
            fprintf(df, "[dump] %s: S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                    nrm_name, n, D, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
            {
                float ss = 0.f; for (int i = 0; i < D; i++) ss += col[i] * col[i];
                float rcpu = 1.f / sqrtf(ss / (float)D + c->eps);
                float rgpu = (w[0] != 0.f && col[0] != 0.f) ? gn[0] / (col[0] * w[0]) : 0.f;
                fprintf(df, "[dump] %s r (GPU y/(x*w) vs CPU rms r of GPU col): r_gpu=%.9e r_cpu=%.9e |diff|=%.3e ss=%.9e\n",
                        nrm_name, rgpu, rcpu, fabsf(rgpu - rcpu), ss);
            }
            if (nrm_n != 0) { fflush(df); }
            else {
            size_t nx = (size_t)n * (size_t)HD, nmix = (size_t)n * (size_t)nm;
            float *xs = malloc(nx * sizeof(float)), *mg = malloc(nmix * sizeof(float)), *mcpu = malloc(nmix * sizeof(float));
            int rd2 = xs && mg && mcpu && vkc_read(ch->xs, 0, xs, nx * 4) && vkc_read(ch->mix, 0, mg, nmix * 4);
            if (!rd2) fprintf(df, "[dump] first-layer mix fmt 10: not sampled\n");
            else {
                matmul(mcpu, xs, m->layer[0].hc_attn_fn, n, HD, nm);
                double dm = 0, mmx = 0; int x_not_f16 = 0;
                for (size_t k = 0; k < nx; k++) {
                    uint32_t u; memcpy(&u, &xs[k], 4);
                    if ((u & 0x1fffu) && (u & 0x7fffffff)) x_not_f16++;
                }
                for (size_t k = 0; k < nmix; k++) {
                    double a = fabs((double)mg[k] - (double)mcpu[k]); if (a > dm) dm = a;
                    double b = fabs((double)mcpu[k]); if (b > mmx) mmx = b;
                }
                fprintf(df, "[dump] first-layer mix fmt 10: S=%d I=%d O=%d x_bytes=4 x_not_f16=%d\n", n, HD, nm, x_not_f16);
                fprintf(df, "[dump] first-layer mix CPU matmul vs device y: max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                        dm, mmx, mmx > 0 ? dm / mmx : 0, 1e-4 * mmx);
                float *ccol = malloc(nd * sizeof(float)), *cpost = malloc((size_t)n * H * sizeof(float));
                float *ccomb = malloc((size_t)n * H * H * sizeof(float));
                if (ccol && cpost && ccomb) {
                    for (int t = 0; t < n; t++)
                        coli_hc_pre(ccol + (size_t)t * D, cpost + (size_t)t * H,
                                    ccomb + (size_t)t * H * H, xs + (size_t)t * HD,
                                    m->layer[0].hc_attn_fn, m->layer[0].hc_attn_scale,
                                    m->layer[0].hc_attn_base, H, D, c->hc_iters, c->eps, c->hc_eps);
                    double dc = 0, mcc = 0;
                    for (size_t k = 0; k < nd; k++) {
                        double a = fabs((double)col[k] - (double)ccol[k]); if (a > dc) dc = a;
                        double b = fabs((double)ccol[k]); if (b > mcc) mcc = b;
                    }
                    fprintf(df, "[dump] first-layer mhc col (GPU col vs CPU coli_hc_pre of GPU xs): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            n, D, dc, mcc, mcc > 0 ? dc / mcc : 0, 1e-4 * mcc);
                    float *ghp = malloc((size_t)n * hr * sizeof(float));
                    if (ghp && vkc_read(ch->hp, 0, ghp, (size_t)n * hr * 4)) {
                        double dp = 0, mp = 0, dcb = 0, mcb = 0;
                        for (int t = 0; t < n; t++) {
                            const float *gp = ghp + (size_t)t * hr, *cp = cpost + (size_t)t * H, *cc = ccomb + (size_t)t * H * H;
                            for (int j = 0; j < H; j++) {
                                double a = fabs((double)gp[H + j] - (double)cp[j]); if (a > dp) dp = a;
                                double b = fabs((double)cp[j]); if (b > mp) mp = b;
                            }
                            for (int j = 0; j < H * H; j++) {
                                double a = fabs((double)gp[2 * H + j] - (double)cc[j]); if (a > dcb) dcb = a;
                                double b = fabs((double)cc[j]); if (b > mcb) mcb = b;
                            }
                        }
                        fprintf(df, "[dump] first-layer mhc post-coeff (GPU hp vs CPU coli_hc_pre): max |diff| %.2e of %.2e rel=%.3e\n",
                                dp, mp, mp > 0 ? dp / mp : 0);
                        fprintf(df, "[dump] first-layer mhc comb (GPU hp vs CPU coli_hc_pre): max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                                dcb, mcb, mcb > 0 ? dcb / mcb : 0, 1e-4 * mcb);
                    }
                    free(ghp);
                }
                free(ccol); free(cpost); free(ccomb);
            }
            free(xs); free(mg); free(mcpu);
            fflush(df);
            }
        }
        free(col); free(gn); free(cn);
        if (!vkc_begin()) ok = 0;
        }
        nrm_n++;
    }
    return ok;
}
/* mHC's exit: xn = mix(xs) + post * br, then the two swap */
static int g53c_post(G53Chain *ch, const GModel *m, int n) {
    const Cfg *c = &m->c; int H = c->hc_mult, D = c->hidden, HD = H * D, hr = 2 * H + H * H;
    VkcMhc po = {n, H, D, c->hc_iters, 0, HD, 0, D, 0, hr, 0, HD, 0, 0, c->eps, c->hc_eps, 0.f};
    static int post_n;
    FILE *df = g53c_dump_fp();
    /* first attn POST, last-layer attn POST, last-layer ffn POST */
    int npost = 2 * c->n_layers, pn = post_n;
    int dump = df && n > 1 && (pn == 0 || pn == npost - 2 || pn == npost - 1);
    const char *post_name = pn == 0 ? "first-layer" : (pn == npost - 2 ? "last-layer-attn" : "last-layer");
    int layer_i = pn / 2, site = pn % 2;
    float *xs = NULL, *br = NULL, *hp = NULL, *gxn = NULL, *cxn = NULL;
    if (dump) {
        xs = malloc((size_t)n * HD * sizeof(float));
        br = malloc((size_t)n * D * sizeof(float));
        hp = malloc((size_t)n * hr * sizeof(float));
        gxn = malloc((size_t)n * HD * sizeof(float));
        cxn = malloc((size_t)n * HD * sizeof(float));
        if (!(xs && br && hp && gxn && cxn &&
              vkc_read(ch->xs, 0, xs, (size_t)n * HD * 4) &&
              vkc_read(ch->br, 0, br, (size_t)n * D * 4) &&
              vkc_read(ch->hp, 0, hp, (size_t)n * hr * 4) &&
              vkc_begin())) dump = 0;
    }
    if (!vkc_mhc(VKC_MHC_POST, ch->xs, ch->br, ch->hp, NULL, ch->xn, &po)) {
        free(xs); free(br); free(hp); free(gxn); free(cxn);
        return 0;
    }
    post_n++;
    if (dump) {
        int rd = vkc_read(ch->xn, 0, gxn, (size_t)n * HD * 4);
        if (!rd) fprintf(df, "[dump] %s mhc post: not sampled\n", post_name);
        else {
            for (int t = 0; t < n; t++)
                coli_hc_post(cxn + (size_t)t * HD, br + (size_t)t * D, xs + (size_t)t * HD,
                             hp + (size_t)t * hr + H, hp + (size_t)t * hr + 2 * H, H, D);
            double d = 0, mc = 0, d64 = 0, m64 = 0;
            for (size_t k = 0; k < (size_t)n * HD; k++) {
                double a = fabs((double)gxn[k] - (double)cxn[k]); if (a > d) d = a;
                double b = fabs((double)cxn[k]); if (b > mc) mc = b;
            }
            /* f32 sequential mix vs f64 mix of the same GPU xs/br/hp */
            for (int t = 0; t < n; t++) {
                const float *res = xs + (size_t)t * HD, *branch = br + (size_t)t * D;
                const float *post = hp + (size_t)t * hr + H, *comb = hp + (size_t)t * hr + 2 * H;
                for (int dest = 0; dest < H; dest++) for (int col = 0; col < D; col++) {
                    float vf = 0.f; double vd = 0;
                    for (int src = 0; src < H; src++) {
                        vf += comb[src * H + dest] * res[src * D + col];
                        vd += (double)comb[src * H + dest] * (double)res[src * D + col];
                    }
                    vf += post[dest] * branch[col];
                    vd += (double)post[dest] * (double)branch[col];
                    double a = fabs((double)vf - vd); if (a > d64) d64 = a;
                    double b = fabs(vd); if (b > m64) m64 = b;
                }
            }
            fprintf(df, "[dump] %s mhc post (GPU xn vs CPU coli_hc_post): S=%d HD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                    post_name, n, HD, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
            fprintf(df, "[dump] %s residual mix f32 sequential vs f64 (same GPU xs/br/hp): max |diff| %.2e of %.2e rel=%.3e\n",
                    post_name, d64, m64, m64 > 0 ? d64 / m64 : 0);
            /* same-input PRE of GPU xs vs GPU hp (mix matrix / post coeffs at this POST) */
            if (layer_i >= 0 && layer_i < c->n_layers) {
                const GLayer *pl = &m->layer[layer_i];
                const float *fn = site ? pl->hc_ffn_fn : pl->hc_attn_fn;
                const float *sc = site ? pl->hc_ffn_scale : pl->hc_attn_scale;
                const float *ba = site ? pl->hc_ffn_base : pl->hc_attn_base;
                float *ccol = malloc((size_t)n * D * sizeof(float));
                float *cpost = malloc((size_t)n * H * sizeof(float));
                float *ccomb = malloc((size_t)n * H * H * sizeof(float));
                if (ccol && cpost && ccomb && fn && sc && ba) {
                    for (int t = 0; t < n; t++)
                        coli_hc_pre(ccol + (size_t)t * D, cpost + (size_t)t * H,
                                    ccomb + (size_t)t * H * H, xs + (size_t)t * HD,
                                    fn, sc, ba, H, D, c->hc_iters, c->eps, c->hc_eps);
                    double dp = 0, mp = 0, dcb = 0, mcb = 0;
                    for (int t = 0; t < n; t++) {
                        const float *gp = hp + (size_t)t * hr, *cp = cpost + (size_t)t * H, *cc = ccomb + (size_t)t * H * H;
                        for (int j = 0; j < H; j++) {
                            double a = fabs((double)gp[H + j] - (double)cp[j]); if (a > dp) dp = a;
                            double b = fabs((double)cp[j]); if (b > mp) mp = b;
                        }
                        for (int j = 0; j < H * H; j++) {
                            double a = fabs((double)gp[2 * H + j] - (double)cc[j]); if (a > dcb) dcb = a;
                            double b = fabs((double)cc[j]); if (b > mcb) mcb = b;
                        }
                    }
                    fprintf(df, "[dump] %s mhc post-coeff same-input (GPU hp vs CPU coli_hc_pre of GPU xs): max |diff| %.2e of %.2e rel=%.3e\n",
                            post_name, dp, mp, mp > 0 ? dp / mp : 0);
                    fprintf(df, "[dump] %s mhc comb same-input (GPU hp vs CPU coli_hc_pre of GPU xs): max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            post_name, dcb, mcb, mcb > 0 ? dcb / mcb : 0, 1e-4 * mcb);
                }
                free(ccol); free(cpost); free(ccomb);
            }
            /* already-diverged streams: CPU replay of the original input to this POST */
            if ((pn == npost - 2 || pn == npost - 1) && g53c_dump_xin && g53c_dump_nr == n) {
                int wide = c->dense_inter > c->moe_inter ? c->dense_inter : c->moe_inter;
                float *cres = malloc((size_t)n * HD * sizeof(float));
                float *cbr = malloc((size_t)n * D * sizeof(float));
                float *cpost = malloc((size_t)n * H * sizeof(float));
                float *ccomb = malloc((size_t)n * H * H * sizeof(float));
                float *cnxt = malloc((size_t)n * HD * sizeof(float));
                float *ccol = malloc((size_t)n * D * sizeof(float));
                float *cnrm = malloc((size_t)n * D * sizeof(float));
                float *csg = malloc((size_t)n * wide * sizeof(float));
                float *csu = malloc((size_t)n * wide * sizeof(float));
                float *cxn2 = malloc((size_t)n * HD * sizeof(float));
                int okp = cres && cbr && cpost && ccomb && cnxt && ccol && cnrm && csg && csu && cxn2 &&
                          g53c_dump_cpu_to_post(m, g53c_dump_xin, n, g53c_dump_start, g53c_dump_lo,
                                                layer_i, site, cres, cbr, cpost, ccomb,
                                                cnxt, ccol, cnrm, csg, csu);
                if (!okp) fprintf(df, "[dump] %s POST internals of diverged streams: not sampled\n", post_name);
                else {
                    g53c_dump_stream_rms_mean(df, post_name, n, H, D, c->eps, xs, cres);
                    {
                        char tag[160];
                        snprintf(tag, sizeof tag, "%s residual streams (GPU xs vs CPU replay at POST)", post_name);
                        g53c_dump_maxdiff(df, tag, xs, cres, (size_t)n * HD);
                        snprintf(tag, sizeof tag, "%s branch (GPU br vs CPU replay at POST)", post_name);
                        g53c_dump_maxdiff(df, tag, br, cbr, (size_t)n * D);
                    }
                    if (pn == npost - 1) {
                        float *gnrm = malloc((size_t)n * D * sizeof(float));
                        float *gcol = malloc((size_t)n * D * sizeof(float));
                        float *cnrm_si = malloc((size_t)n * D * sizeof(float));
                        int rdn = gnrm && gcol && cnrm_si &&
                                  vkc_read(ch->nrm, 0, gnrm, (size_t)n * D * 4) &&
                                  vkc_read(ch->col, 0, gcol, (size_t)n * D * 4);
                        if (!rdn) fprintf(df, "[dump] last-layer nrm into FFN: not sampled\n");
                        else {
                            const float *wln = m->layer[layer_i].post_ln;
                            for (int t = 0; t < n; t++)
                                rms(cnrm_si + (size_t)t * D, gcol + (size_t)t * D, wln, D, c->eps);
                            g53c_dump_maxdiff(df, "last-layer nrm into FFN (GPU nrm vs CPU rms of GPU col)",
                                              gnrm, cnrm_si, (size_t)n * D);
                            g53c_dump_maxdiff(df, "last-layer nrm into FFN (GPU nrm vs CPU replay nrm)",
                                              gnrm, cnrm, (size_t)n * D);
                        }
                        free(gnrm); free(gcol); free(cnrm_si);
                    }
                    double dp = 0, mp = 0, dcb = 0, mcb = 0;
                    for (int t = 0; t < n; t++) {
                        const float *gp = hp + (size_t)t * hr, *cp = cpost + (size_t)t * H, *cc = ccomb + (size_t)t * H * H;
                        for (int j = 0; j < H; j++) {
                            double a = fabs((double)gp[H + j] - (double)cp[j]); if (a > dp) dp = a;
                            double b = fabs((double)cp[j]); if (b > mp) mp = b;
                        }
                        for (int j = 0; j < H * H; j++) {
                            double a = fabs((double)gp[2 * H + j] - (double)cc[j]); if (a > dcb) dcb = a;
                            double b = fabs((double)cc[j]); if (b > mcb) mcb = b;
                        }
                    }
                    fprintf(df, "[dump] %s mhc post-coeff diverged (GPU hp vs CPU coli_hc_pre of CPU residual): max |diff| %.2e of %.2e rel=%.3e\n",
                            post_name, dp, mp, mp > 0 ? dp / mp : 0);
                    fprintf(df, "[dump] %s mhc comb diverged (GPU hp vs CPU coli_hc_pre of CPU residual): max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            post_name, dcb, mcb, mcb > 0 ? dcb / mcb : 0, 1e-4 * mcb);
                    for (int t = 0; t < n; t++)
                        coli_hc_post(cxn2 + (size_t)t * HD, cbr + (size_t)t * D, cres + (size_t)t * HD,
                                     cpost + (size_t)t * H, ccomb + (size_t)t * H * H, H, D);
                    {
                        char tag[160];
                        snprintf(tag, sizeof tag, "%s mhc post (GPU xn vs CPU coli_hc_post of CPU residual/br/hp)", post_name);
                        g53c_dump_maxdiff(df, tag, gxn, cxn2, (size_t)n * HD);
                    }
                }
                free(cres); free(cbr); free(cpost); free(ccomb); free(cnxt);
                free(ccol); free(cnrm); free(csg); free(csu); free(cxn2);
            }
            if (post_n != 1) { fflush(df); }
            else {
                float *ccol = malloc((size_t)n * D * sizeof(float));
                float *cpost2 = malloc((size_t)n * H * sizeof(float));
                float *ccomb2 = malloc((size_t)n * H * H * sizeof(float));
                float *cnrm = malloc((size_t)n * D * sizeof(float));
                float *cbr = malloc((size_t)n * D * sizeof(float));
                float *cxn2 = malloc((size_t)n * HD * sizeof(float));
                size_t ns = (size_t)c->kda_heads * c->kda_hd * c->kda_hd, nw = (size_t)3 * c->kda_proj * c->conv_k;
                float *st = calloc(ns, sizeof(float)), *win = calloc(nw, sizeof(float));
                float *scr = malloc((size_t)coli_kda_scratch_floats(c->kda_heads, c->kda_hd, c->kda_hd) * sizeof(float));
                if (ccol && cpost2 && ccomb2 && cnrm && cbr && cxn2 && st && win && scr) {
                    for (int t = 0; t < n; t++) {
                        coli_hc_pre(ccol + (size_t)t * D, cpost2 + (size_t)t * H,
                                    ccomb2 + (size_t)t * H * H, xs + (size_t)t * HD,
                                    m->layer[0].hc_attn_fn, m->layer[0].hc_attn_scale,
                                    m->layer[0].hc_attn_base, H, D, c->hc_iters, c->eps, c->hc_eps);
                        rms(cnrm + (size_t)t * D, ccol + (size_t)t * D, m->layer[0].in_ln, D, c->eps);
                    }
                    kda_layer(c, &m->layer[0], cnrm, n, cbr, st, win, scr);
                    for (int t = 0; t < n; t++)
                        coli_hc_post(cxn2 + (size_t)t * HD, cbr + (size_t)t * D, xs + (size_t)t * HD,
                                     cpost2 + (size_t)t * H, ccomb2 + (size_t)t * H * H, H, D);
                    double d2 = 0, mc2 = 0;
                    for (size_t k = 0; k < (size_t)n * HD; k++) {
                        double a = fabs((double)gxn[k] - (double)cxn2[k]); if (a > d2) d2 = a;
                        double b = fabs((double)cxn2[k]); if (b > mc2) mc2 = b;
                    }
                    fprintf(df, "[dump] first-layer composed streams (GPU xn vs CPU site of GPU xs): S=%d HD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            n, HD, d2, mc2, mc2 > 0 ? d2 / mc2 : 0, 1e-4 * mc2);
                }
                free(ccol); free(cpost2); free(ccomb2); free(cnrm); free(cbr); free(cxn2);
                free(st); free(win); free(scr);
            }
            fflush(df);
        }
        if (!vkc_begin()) { free(xs); free(br); free(hp); free(gxn); free(cxn); return 0; }
    }
    free(xs); free(br); free(hp); free(gxn); free(cxn);
    VkcBuf *t = ch->xs; ch->xs = ch->xn; ch->xn = t;
    return 1;
}
/* COLI_VK_DUMP_FMT1: CPU matmul_q vs the device y of one chain fmt 1 matmul. */
static FILE *g53c_dump_fp(void) {
    const char *de = getenv("COLI_VK_DUMP_FMT1");
    static FILE *df; static int opened;
    if (!de || !*de || *de == '0') return NULL;
    if (!opened) { opened = 1; df = (*de == '/') ? fopen(de, "w") : NULL; if (!df) df = stderr; }
    return df;
}
static void g53c_dump_fmt1(const char *name, const Mat *w, VkcBuf *x, size_t xo, VkcBuf *y, size_t yo, int n) {
    FILE *df = g53c_dump_fp();
    if (!df) return;
    if (!w || w->fmt != 1 || !w->q8 || !w->s) {
        fprintf(df, "[dump] %s: skip fmt=%d q8=%d s=%d\n", name, w ? w->fmt : -1, w && w->q8 ? 1 : 0, w && w->s ? 1 : 0);
        return;
    }
    int I = w->columns, O = w->rows;
    size_t nx = (size_t)n * (size_t)I, ny = (size_t)n * (size_t)O;
    float *xh = malloc(nx * sizeof(float)), *yh = malloc(ny * sizeof(float)), *yc = malloc(ny * sizeof(float));
    int rd = xh && yh && yc && vkc_read(x, xo, xh, nx * 4) && vkc_read(y, yo, yh, ny * 4);
    if (!rd) fprintf(df, "[dump] %s: not sampled (read failed) S=%d I=%d O=%d\n", name, n, I, O);
    else {
        matmul_q(yc, xh, w->q8, w->s, n, I, O);
        double d = 0, my = 0, mx = 0, mc = 0; int x_not_f16 = 0;
        for (size_t k = 0; k < nx; k++) {
            double ax = fabs((double)xh[k]); if (ax > mx) mx = ax;
            uint32_t u; memcpy(&u, &xh[k], 4);
            if ((u & 0x1fffu) && (u & 0x7fffffff)) x_not_f16++;
        }
        for (size_t k = 0; k < ny; k++) {
            double a = fabs((double)yh[k] - (double)yc[k]); if (a > d) d = a;
            double b = fabs((double)yc[k]); if (b > mc) mc = b;
            double gy = fabs((double)yh[k]); if (gy > my) my = gy;
        }
        fprintf(df, "[dump] %s: S=%d I=%d O=%d x_bytes=4 x_not_f16=%d max|x|=%.2e\n", name, n, I, O, x_not_f16, mx);
        fprintf(df, "[dump] %s CPU matmul_q vs device y: max |diff| %.2e of %.2e (cpu max) gpu_max=%.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                name, d, mc, my, mc > 0 ? d / mc : 0, 1e-4 * mc);
        fflush(df);
    }
    free(xh); free(yh); free(yc);
}
static int g53c_mlp(G53Chain *ch, const GModel *m, const Mat *g, const Mat *u, const Mat *d, VkcBuf *out, int n) {
    int I = g->rows;
    VkcMhc sw = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, n * I, 0.f, 0.f, m->c.swiglu_limit};
    static int mlp_dumped;
    int dump = g53c_dump_fp() != NULL && !mlp_dumped;
    int ok = vkc_matmul(g53c_tensor(g), ch->nrm, 0, ch->gs, 0, n);
    if (ok && dump) { g53c_dump_fmt1("first-layer fmt 1 mlp_g", g, ch->nrm, 0, ch->gs, 0, n); if (!vkc_begin()) ok = 0; }
    ok = ok && vkc_matmul(g53c_tensor(u), ch->nrm, 0, ch->us, 0, n) &&
         vkc_mhc(VKC_SWIGLU_CLAMP, ch->gs, ch->us, NULL, NULL, ch->hs, &sw) &&
         vkc_matmul(g53c_tensor(d), ch->hs, 0, out, 0, n);
    if (ok && dump) { g53c_dump_fmt1("first-layer fmt 1 mlp_d", d, ch->hs, 0, out, 0, n); mlp_dumped = 1; if (!vkc_begin()) ok = 0; }
    return ok;
}
/* COLI_VK_DUMP_FMT1: KDA layer i of the same GPU nrm (CPU kda_layer / conv / ko). */
static void g53c_dump_kda_same(G53Chain *ch, const GModel *m, const GLayer *l, int i, int n) {
    FILE *df = g53c_dump_fp();
    if (!df || n <= 1) return;
    const Cfg *c = &m->c;
    int P = c->kda_proj, HK = c->kda_heads, KD = c->kda_hd, r = ch->rows, KW = c->conv_k, D = c->hidden;
    size_t ns = (size_t)HK * KD * KD, nw = (size_t)3 * P * KW, nb = (size_t)n * D, ny = (size_t)n * P;
    Mat *mats[9] = {(Mat *)&l->kq, (Mat *)&l->kk, (Mat *)&l->kv, (Mat *)&l->ko,
                    (Mat *)&l->kga, (Mat *)&l->kgb, (Mat *)&l->kfa, (Mat *)&l->kfb, (Mat *)&l->kb};
    for (int k = 0; k < 9; k++) if (g53_dho_gone(mats[k])) g53_dho_reload(mats[k]);
    float *st = calloc(ns, sizeof(float)), *win = calloc(nw, sizeof(float));
    float *scr = malloc((size_t)coli_kda_scratch_floats(HK, KD, KD) * sizeof(float));
    float *xn = malloc((size_t)n * D * sizeof(float));
    float *gbr = malloc(nb * sizeof(float)), *cbr = malloc(nb * sizeof(float));
    float *gst = malloc(ns * sizeof(float)), *gwin = malloc(nw * sizeof(float));
    float *gky = malloc(ny * sizeof(float)), *cko = malloc(nb * sizeof(float));
    float *gcm = malloc((size_t)n * 3 * (size_t)P * sizeof(float));
    float *gqkv = malloc((size_t)3 * (size_t)r * P * sizeof(float));
    float *cwin = calloc(nw, sizeof(float)), *ccm = malloc((size_t)n * 3 * (size_t)P * sizeof(float));
    int rd = st && win && scr && xn && gbr && cbr && gst && gwin && gky && cko && gcm && gqkv && cwin && ccm &&
             vkc_read(ch->nrm, 0, xn, (size_t)n * D * 4) &&
             vkc_read(ch->br, 0, gbr, nb * 4) &&
             vkc_read(ch->st[i], 0, gst, ns * 4) &&
             vkc_read(ch->win[i], 0, gwin, nw * 4) &&
             vkc_read(ch->ky, 0, gky, ny * 4) &&
             vkc_read(ch->cm, 0, gcm, (size_t)n * 3 * (size_t)P * 4) &&
             vkc_read(ch->qkv3, 0, gqkv, (size_t)3 * (size_t)r * P * 4);
    if (!rd) fprintf(df, "[dump] layer%d kda same-input: not sampled\n", i);
    else {
        kda_layer(c, l, xn, n, cbr, st, win, scr);
        double d = 0, mc = 0, ds = 0, ms = 0, dw = 0, mw = 0, dk = 0, mk = 0, dc = 0, mcv = 0;
        for (size_t k = 0; k < nb; k++) {
            double a = fabs((double)gbr[k] - (double)cbr[k]); if (a > d) d = a;
            double b = fabs((double)cbr[k]); if (b > mc) mc = b;
        }
        for (size_t k = 0; k < ns; k++) {
            double a = fabs((double)gst[k] - (double)st[k]); if (a > ds) ds = a;
            double b = fabs((double)st[k]); if (b > ms) ms = b;
        }
        for (size_t k = 0; k < nw; k++) {
            double a = fabs((double)gwin[k] - (double)win[k]); if (a > dw) dw = a;
            double b = fabs((double)win[k]); if (b > mw) mw = b;
        }
        fprintf(df, "[dump] layer%d kda composed (GPU br vs CPU kda_layer of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                i, n, D, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
        fprintf(df, "[dump] layer%d kda state (GPU st vs CPU kda_layer of GPU nrm): H=%d KD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                i, HK, KD, ds, ms, ms > 0 ? ds / ms : 0, 1e-4 * ms);
        fprintf(df, "[dump] layer%d kda window (GPU win vs CPU kda_layer of GPU nrm): C=%d K=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                i, 3 * P, KW, dw, mw, mw > 0 ? dw / mw : 0, 1e-4 * mw);
        mm(cko, &l->ko, gky, n);
        for (size_t k = 0; k < nb; k++) {
            double a = fabs((double)gbr[k] - (double)cko[k]); if (a > dk) dk = a;
            double b = fabs((double)cko[k]); if (b > mk) mk = b;
        }
        fprintf(df, "[dump] layer%d kda ko (GPU br vs CPU mm(ko) of GPU ky): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                i, n, D, dk, mk, mk > 0 ? dk / mk : 0, 1e-4 * mk);
        for (int s = 0; s < n; s++) {
            for (int chn = 0; chn < 3 * P; chn++) {
                int part = chn / P, j = chn - part * P;
                float *history = cwin + (size_t)chn * KW;
                if (KW > 1) memmove(history, history + 1, (size_t)(KW - 1) * sizeof(float));
                history[KW - 1] = gqkv[(size_t)part * r * P + (size_t)s * P + j];
                const float *taps = l->conv + (size_t)chn * KW;
                float sum = 0.f;
                for (int tap = 0; tap < KW; tap++) sum += taps[tap] * history[tap];
                ccm[(size_t)s * 3 * P + chn] = coli_kda_silu(sum);
            }
        }
        for (size_t k = 0; k < (size_t)n * 3 * (size_t)P; k++) {
            double a = fabs((double)gcm[k] - (double)ccm[k]); if (a > dc) dc = a;
            double b = fabs((double)ccm[k]); if (b > mcv) mcv = b;
        }
        fprintf(df, "[dump] layer%d kda conv (GPU cm vs CPU silu-conv of GPU qkv): S=%d C=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                i, n, 3 * P, dc, mcv, mcv > 0 ? dc / mcv : 0, 1e-4 * mcv);
        fflush(df);
    }
    free(st); free(win); free(scr); free(xn); free(gbr); free(cbr);
    free(gst); free(gwin); free(gky); free(cko); free(gcm); free(gqkv); free(cwin); free(ccm);
}
static int g53c_kda(G53Chain *ch, const GModel *m, const GLayer *l, int i, int n) {
    const Cfg *c = &m->c; int P = c->kda_proj, HK = c->kda_heads, KD = c->kda_hd, r = ch->rows;
    VkcKdaConv cp = {n, 3 * P, c->conv_k, P, 0, P, r * P, 0, 3 * P, (int)ch->o_conv[i], 0};
    VkcKdaRec rp = {n, HK, KD, P, 0, 3 * P, 0, P, 0, HK, 0, P, 0, P, 0, (int)ch->o_kda[i], c->gate_lb, 1e-6f, c->eps};
    int dump = (i == 0) && g53c_dump_fp() != NULL;
    int dump_kda = (i <= 2) && g53c_dump_fp() != NULL && n > 1;
    int ok = vkc_matmul(g53c_tensor(&l->kq), ch->nrm, 0, ch->qkv3, 0, n);
    if (ok && dump) { g53c_dump_fmt1("first-layer fmt 1 kq", &l->kq, ch->nrm, 0, ch->qkv3, 0, n); if (!vkc_begin()) ok = 0; }
    ok = ok && vkc_matmul(g53c_tensor(&l->kk), ch->nrm, 0, ch->qkv3, (size_t)r * P, n) &&
         vkc_matmul(g53c_tensor(&l->kv), ch->nrm, 0, ch->qkv3, (size_t)2 * r * P, n) &&
         vkc_matmul(g53c_tensor(&l->kfa), ch->nrm, 0, ch->klow, 0, n);
    if (ok && dump) { g53c_dump_fmt1("first-layer fmt 1 kfa", &l->kfa, ch->nrm, 0, ch->klow, 0, n); if (!vkc_begin()) ok = 0; }
    ok = ok && vkc_matmul(g53c_tensor(&l->kfb), ch->klow, 0, ch->kf, 0, n) &&
         vkc_matmul(g53c_tensor(&l->kb), ch->nrm, 0, ch->kb, 0, n) &&
         vkc_matmul(g53c_tensor(&l->kga), ch->nrm, 0, ch->klow, 0, n) &&
         vkc_matmul(g53c_tensor(&l->kgb), ch->klow, 0, ch->kg, 0, n) &&
         vkc_kda_conv(ch->qkv3, ch->prm, ch->win[i], ch->cm, &cp) &&
         vkc_kda_rec(KD, ch->cm, ch->kf, ch->kb, ch->kg, ch->prm, ch->st[i], ch->ky, &rp) &&
         vkc_matmul(g53c_tensor(&l->ko), ch->ky, 0, ch->br, 0, n);
    if (ok && (dump || dump_kda)) {
        if (dump) g53c_dump_fmt1("first-layer fmt 1 ko", &l->ko, ch->ky, 0, ch->br, 0, n);
        if (dump_kda) g53c_dump_kda_same(ch, m, l, i, n);
        if (!vkc_begin()) ok = 0;
    }
    return ok;
}
static int g53c_mla(G53Chain *ch, const GModel *m, const GSession *s, const GLayer *l, int i, int n, int pb) {
    const Cfg *c = &m->c; int IH = c->index_nh, ID = c->index_hd, pool = c->index_kpool, K = c->kv_lora;
    int width = coli_sparse_index_width(c->index_topk, pool, c->index_kpool_tail);
    size_t ko = (size_t)(i - ch->lo) * ch->kvd_layer;
    VkcMlaRow ln = {n, 1, 0, ID, 0, 0, ID, 0, pb * ID, ID, 0, 0, 0, (int)ch->o_ikw[i], (int)ch->o_ikb[i], l->ik_nb != NULL, 1e-5f};
    int done = (pb + n) / pool, p0 = ch->pool_valid[i];
    VkcDsaPool kp = {done > p0 ? done - p0 : 0, pool, p0, ID, 0, ID, (int)ch->o_ape[i], 0, 0, ID};
    VkcDsaPick sp = {n, pb, IH, ID, c->index_topk, pool, 0, IH * ID, 0, IH, 0, c->index_kpool_tail, (pb + n) / pool + 1,
                     1 + width, sqrtf((float)IH), 1.f / sqrtf((float)ID)};
    int dump_last = (i == c->n_layers - 1) && g53c_dump_fp() != NULL;
    int ok = (ch->ks.on ? vkc_kv_mla_qkv(&ch->ks, ch->mla_ord[i], &ch->mla[i], &ch->sc, ch->nrm, 0, n, pb, NULL, &ch->kv[i],
                                         &ch->kvtmp, ch->kvd, ko)
                        : vkc_mla_qkv(&ch->mla[i], &ch->sc, ch->nrm, 0, n, pb, NULL, &ch->kv[i], ch->kvd, ko)) &&
             vkc_matmul(g53c_tensor(&l->iwq), ch->sc.qa, 0, ch->iq, 0, n) &&
             vkc_matmul(g53c_tensor(&l->iwk), ch->nrm, 0, ch->ikd, 0, n);
    if (ok && dump_last) {
        if (g53_dho_gone(&l->iwk)) g53_dho_reload((Mat *)&l->iwk);
        g53c_dump_fmt1("last-layer fmt 1 iwk", &l->iwk, ch->nrm, 0, ch->ikd, 0, n);
        if (!vkc_begin()) ok = 0;
    }
    ok = ok && vkc_mla_lnorm(ch->ikd, ch->prm, ch->ik[i], &ln) &&
             vkc_matmul(g53c_tensor(&l->ikpg), ch->nrm, 0, ch->igd, 0, n) &&
             vkc_copy(ch->ig[i], (size_t)pb * ID, ch->igd, 0, (size_t)n * ID) &&
             vkc_matmul(g53c_tensor(&l->iwp), ch->nrm, 0, ch->ihw, 0, n) &&
             vkc_copy(ch->kvd, ko + (size_t)n * K, ch->ik[i], (size_t)pb * ID, (size_t)n * ID) &&
             vkc_copy(ch->kvd, ko + (size_t)n * (K + ID), ch->igd, 0, (size_t)n * ID) &&
             vkc_dsa_pool_keys(ch->ik[i], ch->ig[i], ch->prm, ch->pk[i], &kp) &&
             vkc_dsa_pool_select(ch->iq, ch->ihw, ch->pk[i], ch->isc, ch->sel, &sp) &&
             (ch->ks.on ? vkc_kv_mla_attn(&ch->ks, ch->mla_ord[i], &ch->mla[i], &ch->sc, n, pb, 0, &ch->kv[i], ch->sel, 0, 1 + width,
                                          NULL, 0, ch->br, 0, s->layer[i].latent, NULL)
                        : vkc_mla_attn(&ch->mla[i], &ch->sc, n, pb, 0, &ch->kv[i], ch->sel, 0, 1 + width, NULL, 0, ch->br, 0));
    if (ok && dump_last && n > 1) {
        FILE *df = g53c_dump_fp();
        size_t nb = (size_t)n * c->hidden, nlat = (size_t)s->cap * (size_t)K, nik = (size_t)s->cap * (size_t)ID;
        float *xn = malloc((size_t)n * c->hidden * sizeof(float));
        float *gbr = malloc(nb * sizeof(float)), *cbr = malloc(nb * sizeof(float));
        GLayerState st = {0};
        st.latent = calloc(nlat, sizeof(float));
        st.ikeys = calloc(nik, sizeof(float));
        st.igates = calloc(nik, sizeof(float));
        float *gikd = malloc((size_t)n * ID * sizeof(float));
        float *gik = malloc((size_t)n * ID * sizeof(float));
        float *cik = malloc((size_t)n * ID * sizeof(float));
        float *giq = malloc((size_t)n * IH * ID * sizeof(float));
        float *gihw = malloc((size_t)n * IH * sizeof(float));
        float *gig = malloc((size_t)n * ID * sizeof(float));
        float *hwsc = malloc((size_t)n * IH * sizeof(float));
        int *gsel = malloc((size_t)n * (1 + width) * sizeof(int));
        int *csel = malloc((size_t)n * width * sizeof(int));
        unsigned char *valid = malloc((size_t)(pb + n));
        int rd = df && xn && gbr && cbr && st.latent && st.ikeys && st.igates &&
                 vkc_read(ch->nrm, 0, xn, (size_t)n * c->hidden * 4) &&
                 vkc_read(ch->br, 0, gbr, nb * 4);
        int rd_ix = rd && gikd && gik && cik && giq && gihw && gig && hwsc && gsel && csel && valid &&
                    vkc_read(ch->ikd, 0, gikd, (size_t)n * ID * 4) &&
                    vkc_read(ch->ik[i], (size_t)pb * ID * 4, gik, (size_t)n * ID * 4) &&
                    vkc_read(ch->iq, 0, giq, (size_t)n * IH * ID * 4) &&
                    vkc_read(ch->ihw, 0, gihw, (size_t)n * IH * 4) &&
                    vkc_read(ch->ig[i], (size_t)pb * ID * 4, gig, (size_t)n * ID * 4) &&
                    vkc_read(ch->sel, 0, gsel, (size_t)n * (1 + width) * sizeof(int));
        if (!df) { /* skip */ }
        else if (!rd) fprintf(df, "[dump] last-layer mla composed: not sampled\n");
        else {
            if (!rd_ix) fprintf(df, "[dump] last-layer indexer LN/sel: not sampled\n");
            else {
                double dln = 0, mln = 0;
                for (int t = 0; t < n; t++)
                    layer_norm(cik + (size_t)t * ID, gikd + (size_t)t * ID, l->ik_nw, l->ik_nb, ID, 1e-5f);
                for (size_t k = 0; k < (size_t)n * ID; k++) {
                    double a = fabs((double)gik[k] - (double)cik[k]); if (a > dln) dln = a;
                    double b = fabs((double)cik[k]); if (b > mln) mln = b;
                }
                fprintf(df, "[dump] last-layer indexer LN (GPU ik vs CPU layer_norm of GPU iwk y): S=%d ID=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                        n, ID, dln, mln, mln > 0 ? dln / mln : 0, 1e-4 * mln);
                memset(valid, 1, (size_t)(pb + n));
                for (int t = 0; t < n; t++)
                    for (int h = 0; h < IH; h++)
                        hwsc[(size_t)t * IH + h] = gihw[(size_t)t * IH + h] / sqrtf((float)IH);
                if (coli_sparse_index_select_range(csel, giq, gik, gig, hwsc, l->ikpa, valid,
                                                   pb + n, IH, ID, pool, c->index_topk,
                                                   c->index_kpool_tail, pb, pb + n))
                    fprintf(df, "[dump] last-layer indexer sel: CPU select failed\n");
                else {
                    int nmis = 0, ncmp = 0, ncnt = 0;
                    for (int t = 0; t < n; t++) {
                        int *grow = gsel + (size_t)t * (1 + width);
                        int *crow = csel + (size_t)t * width;
                        if (grow[0] != width) ncnt++;
                        for (int j = 0; j < width; j++, ncmp++)
                            if (grow[1 + j] != crow[j]) nmis++;
                    }
                    fprintf(df, "[dump] last-layer indexer sel (GPU sel vs CPU of same GPU iq/ik/gates/hw): S=%d width=%d mismatches=%d of %d count_ne_width=%d\n",
                            n, width, nmis, ncmp, ncnt);
                }
            }
            mla_layer(c, l, xn, n, cbr, &st, pb);
            double d = 0, mc = 0;
            for (size_t k = 0; k < nb; k++) {
                double a = fabs((double)gbr[k] - (double)cbr[k]); if (a > d) d = a;
                double b = fabs((double)cbr[k]); if (b > mc) mc = b;
            }
            fprintf(df, "[dump] last-layer mla composed (GPU br vs CPU mla_layer of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                    n, c->hidden, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
            fflush(df);
        }
        free(gikd); free(gik); free(cik); free(giq); free(gihw); free(gig); free(hwsc);
        free(gsel); free(csel); free(valid);
        free(st.latent); free(st.ikeys); free(st.igates); free(xn); free(gbr); free(cbr);
        if (!vkc_begin()) ok = 0;
    }
    if (ok && done > ch->pool_valid[i]) ch->pool_valid[i] = done;
    return ok;
}

/* A device is gone (who: the chain whose device it was); both chains go off. If the
 * devices held s's newest KDA state (dev0, dev2: the primary's and the second device's
 * held it when the forward from `start` began), the state of their layers is rebuilt on
 * the CPU from the recorded input rows of [rec_base, start), from the host's copy at
 * rec_base. The second device's is read back first (g53c_sync_host): dev2 without dev0
 * does not happen. */
static void g53c_recover(GModel *m, GSession *s, int start, int dev0, int dev2, const G53Chain *who) {
    G53Chain *ch = g_g53c, *ch2 = g_g53c2;
    g_vk_chain = 0;
    if (!ch) return;
    int had = ch->owner == s && (dev0 || dev2);
    int hi = ch->lo + ch->n + (dev2 && ch2 ? ch2->n : 0);   /* the layers whose state the devices held */
    for (int d = 0; d < 2; d++) {
        G53Chain *x = g53c_of(d);
        if (x) { x->failed = 1; x->owner = NULL; x->where = G53C_HOST; }
    }
    const char *nm = g53c_name(who);
    if (!had) {
        fprintf(stderr, "[VK] %s chain: the device was lost; the host's state is current, the CPU runs from here on\n", nm);
        return;
    }
    const Cfg *c = &m->c; int H = c->hc_mult, D = c->hidden;
    if (!ch->rec_ok || ch->rec_base + ch->rec_len < start || !dev0) {
        fprintf(stderr, "[VK] %s chain: the device was lost with a KDA state its recorded rows do not describe -- stopping "
                        "(COLI_VK_CHAIN=0 keeps the state on the CPU)\n", nm);
        exit(1);
    }
    int n = start - ch->rec_base;
    fprintf(stderr, "[VK] %s chain: the device was lost; rebuilding the state of %d positions on the CPU, "
                    "which runs from here on\n", nm, n);
    /* the devices' layers only: a partial chain's CPU layers ran every position already */
    if (n <= 0) return;
    float *xs = malloc((size_t)n * H * D * sizeof(float)), *xn = malloc((size_t)n * H * D * sizeof(float));
    if (!xs || !xn) { fprintf(stderr, "OOM rebuilding the state\n"); exit(1); }
    for (int t = 0; t < n; t++) for (int h = 0; h < H; h++)
        memcpy(xs + ((size_t)t * H + h) * D, ch->rec + (size_t)t * D, (size_t)D * sizeof(float));
    run_layers(m, s, xs, xn, n, ch->rec_base, 0, hi);
    free(xs); free(xn);
}

/* COLI_VK_DUMP_FMT1: CPU-replay layers lo..upto of `xin` into `xs` (result in xs). */
static int g53c_dump_cpu_layers(const GModel *m, float *xs, const float *xin, int nr, int start,
                                int lo, int upto, float *nxt, float *col, float *post, float *comb,
                                float *nrm, float *br, float *sg, float *su) {
    const Cfg *c = &m->c; int D = c->hidden, H = c->hc_mult, HD = H * D;
    memcpy(xs, xin, (size_t)nr * HD * sizeof(float));
    size_t ns = (size_t)c->kda_heads * c->kda_hd * c->kda_hd, nw = (size_t)3 * c->kda_proj * c->conv_k;
    float *scratch = malloc((size_t)coli_kda_scratch_floats(c->kda_heads, c->kda_hd, c->kda_hd) * sizeof(float));
    GLayerState *st = calloc((size_t)c->n_layers, sizeof(*st));
    int ok = scratch && st;
    for (int i = lo; i <= upto && ok; i++) {
        if (c->is_full[i]) {
            st[i].latent = calloc((size_t)(start + nr) * c->kv_lora, sizeof(float));
            st[i].ikeys = calloc((size_t)(start + nr) * c->index_hd, sizeof(float));
            st[i].igates = calloc((size_t)(start + nr) * c->index_hd, sizeof(float));
            ok = st[i].latent && st[i].ikeys && st[i].igates;
        } else if (c->kda_proj) {
            st[i].kda_state = calloc(ns, sizeof(float));
            st[i].kda_window = calloc(nw, sizeof(float));
            ok = st[i].kda_state && st[i].kda_window;
        }
    }
    for (int i = lo; i <= upto && ok; i++) {
        const GLayer *l = &m->layer[i];
        for (int t = 0; t < nr; t++) {
            coli_hc_pre(col + (size_t)t * D, post + (size_t)t * H, comb + (size_t)t * H * H,
                        xs + (size_t)t * HD, l->hc_attn_fn, l->hc_attn_scale, l->hc_attn_base,
                        H, D, c->hc_iters, c->eps, c->hc_eps);
            rms(nrm + (size_t)t * D, col + (size_t)t * D, l->in_ln, D, c->eps);
        }
        if (c->is_full[i]) mla_layer(c, l, nrm, nr, br, &st[i], start);
        else kda_layer(c, l, nrm, nr, br, st[i].kda_state, st[i].kda_window, scratch);
        for (int t = 0; t < nr; t++)
            coli_hc_post(nxt + (size_t)t * HD, br + (size_t)t * D, xs + (size_t)t * HD,
                         post + (size_t)t * H, comb + (size_t)t * H * H, H, D);
        for (int t = 0; t < nr; t++) {
            coli_hc_pre(col + (size_t)t * D, post + (size_t)t * H, comb + (size_t)t * H * H,
                        nxt + (size_t)t * HD, l->hc_ffn_fn, l->hc_ffn_scale, l->hc_ffn_base,
                        H, D, c->hc_iters, c->eps, c->hc_eps);
            rms(nrm + (size_t)t * D, col + (size_t)t * D, l->post_ln, D, c->eps);
        }
        if (i < c->first_dense) {
            if (g53_dho_gone(&l->dg)) g53_dho_reload((Mat *)&l->dg);
            if (g53_dho_gone(&l->du)) g53_dho_reload((Mat *)&l->du);
            if (g53_dho_gone(&l->dd)) g53_dho_reload((Mat *)&l->dd);
            mlp3_rows(br, nrm, nr, &l->dg, &l->du, &l->dd, c->swiglu_limit, sg, su);
        } else {
            ffn_layer_ex((GModel *)m, l, i, nrm, nr, br, 1, 0);
        }
        for (int t = 0; t < nr; t++)
            coli_hc_post(xs + (size_t)t * HD, br + (size_t)t * D, nxt + (size_t)t * HD,
                         post + (size_t)t * H, comb + (size_t)t * H * H, H, D);
    }
    for (int i = lo; i <= upto; i++) {
        free(st[i].latent); free(st[i].ikeys); free(st[i].igates);
        free(st[i].kda_state); free(st[i].kda_window);
    }
    free(st); free(scratch);
    return ok;
}

/* CPU replay of `xin` through [lo, layer], stopping at POST entry of `site` (0 attn, 1 ffn).
 * residual/branch/post/comb are that POST's CPU inputs. nxt/col/nrm/sg/su are scratch. */
static int g53c_dump_cpu_to_post(const GModel *m, const float *xin, int nr, int start,
                                 int lo, int layer, int site,
                                 float *residual, float *branch, float *post, float *comb,
                                 float *nxt, float *col, float *nrm, float *sg, float *su) {
    const Cfg *c = &m->c; int D = c->hidden, H = c->hc_mult, HD = H * D;
    if (layer < lo || layer >= c->n_layers || site < 0 || site > 1) return 0;
    int ok;
    if (layer > lo)
        ok = g53c_dump_cpu_layers(m, residual, xin, nr, start, lo, layer - 1,
                                  nxt, col, post, comb, nrm, branch, sg, su);
    else {
        memcpy(residual, xin, (size_t)nr * HD * sizeof(float));
        ok = 1;
    }
    if (!ok) return 0;
    const GLayer *l = &m->layer[layer];
    size_t ns = (size_t)c->kda_heads * c->kda_hd * c->kda_hd, nw = (size_t)3 * c->kda_proj * c->conv_k;
    float *scratch = malloc((size_t)coli_kda_scratch_floats(c->kda_heads, c->kda_hd, c->kda_hd) * sizeof(float));
    GLayerState st = {0};
    if (c->is_full[layer]) {
        st.latent = calloc((size_t)(start + nr) * c->kv_lora, sizeof(float));
        st.ikeys = calloc((size_t)(start + nr) * c->index_hd, sizeof(float));
        st.igates = calloc((size_t)(start + nr) * c->index_hd, sizeof(float));
        ok = scratch && st.latent && st.ikeys && st.igates;
    } else if (c->kda_proj) {
        st.kda_state = calloc(ns, sizeof(float));
        st.kda_window = calloc(nw, sizeof(float));
        ok = scratch && st.kda_state && st.kda_window;
    } else ok = scratch != NULL;
    if (ok) {
        for (int t = 0; t < nr; t++) {
            coli_hc_pre(col + (size_t)t * D, post + (size_t)t * H, comb + (size_t)t * H * H,
                        residual + (size_t)t * HD, l->hc_attn_fn, l->hc_attn_scale, l->hc_attn_base,
                        H, D, c->hc_iters, c->eps, c->hc_eps);
            rms(nrm + (size_t)t * D, col + (size_t)t * D, l->in_ln, D, c->eps);
        }
        if (c->is_full[layer]) mla_layer(c, l, nrm, nr, branch, &st, start);
        else kda_layer(c, l, nrm, nr, branch, st.kda_state, st.kda_window, scratch);
    }
    if (ok && site == 1) {
        for (int t = 0; t < nr; t++)
            coli_hc_post(nxt + (size_t)t * HD, branch + (size_t)t * D, residual + (size_t)t * HD,
                         post + (size_t)t * H, comb + (size_t)t * H * H, H, D);
        for (int t = 0; t < nr; t++) {
            coli_hc_pre(col + (size_t)t * D, post + (size_t)t * H, comb + (size_t)t * H * H,
                        nxt + (size_t)t * HD, l->hc_ffn_fn, l->hc_ffn_scale, l->hc_ffn_base,
                        H, D, c->hc_iters, c->eps, c->hc_eps);
            rms(nrm + (size_t)t * D, col + (size_t)t * D, l->post_ln, D, c->eps);
        }
        if (layer < c->first_dense) {
            if (g53_dho_gone(&l->dg)) g53_dho_reload((Mat *)&l->dg);
            if (g53_dho_gone(&l->du)) g53_dho_reload((Mat *)&l->du);
            if (g53_dho_gone(&l->dd)) g53_dho_reload((Mat *)&l->dd);
            mlp3_rows(branch, nrm, nr, &l->dg, &l->du, &l->dd, c->swiglu_limit, sg, su);
        } else {
            ffn_layer_ex((GModel *)m, l, layer, nrm, nr, branch, 1, 0);
        }
        memcpy(residual, nxt, (size_t)nr * HD * sizeof(float));
    }
    free(st.latent); free(st.ikeys); free(st.igates);
    free(st.kda_state); free(st.kda_window);
    free(scratch);
    return ok;
}

/* COLI_VK_DUMP_FMT1: last-layer FFN/silu/experts of GPU nrm vs CPU, and of CPU replay nrm. */
static void g53c_dump_last_ffn_parts(G53Chain *ch, GModel *m, int layer, int nr,
                                    const float *gnrm, const float *gds, const float *grt) {
    FILE *df = g53c_dump_fp();
    if (!df || nr <= 1 || !gnrm || !gds || !grt || layer < 0) return;
    const Cfg *c = &m->c;
    const GLayer *l = &m->layer[layer];
    int D = c->hidden, H = c->hc_mult, HD = H * D, I = l->rg.rows, topk = c->topk, E = c->n_experts;
    int wide = c->dense_inter > c->moe_inter ? c->dense_inter : c->moe_inter;
    size_t nd = (size_t)nr * D, ni = (size_t)nr * I;
    float *ggs = malloc(ni * sizeof(float)), *gus = malloc(ni * sizeof(float)), *ghs = malloc(ni * sizeof(float));
    float *chs = malloc(ni * sizeof(float)), *cds = malloc(nd * sizeof(float)), *crt = malloc(nd * sizeof(float));
    float *sg = malloc((size_t)nr * wide * sizeof(float)), *su = malloc((size_t)nr * wide * sizeof(float));
    int rd = ggs && gus && ghs && chs && cds && crt && sg && su &&
             vkc_read(ch->gs, 0, ggs, ni * 4) && vkc_read(ch->us, 0, gus, ni * 4) &&
             vkc_read(ch->hs, 0, ghs, ni * 4);
    if (!rd) fprintf(df, "[dump] last-layer FFN silu/shared/routed: not sampled\n");
    else {
        memcpy(chs, ggs, ni * sizeof(float));
        for (int t = 0; t < nr; t++)
            swiglu_clamped(chs + (size_t)t * I, gus + (size_t)t * I, I, c->swiglu_limit);
        g53c_dump_maxdiff(df, "last-layer silu of GPU nrm (GPU hs vs CPU swiglu of GPU gs/us)",
                          ghs, chs, ni);
        if (g53_dho_gone(&l->rg)) g53_dho_reload((Mat *)&l->rg);
        if (g53_dho_gone(&l->ru)) g53_dho_reload((Mat *)&l->ru);
        if (g53_dho_gone(&l->rd)) g53_dho_reload((Mat *)&l->rd);
        mlp3_rows(cds, gnrm, nr, &l->rg, &l->ru, &l->rd, c->swiglu_limit, sg, su);
        g53c_dump_maxdiff(df, "last-layer shared of GPU nrm (GPU ds vs CPU mlp3 of GPU nrm)",
                          gds, cds, nd);
        ffn_layer_ex(m, l, layer, gnrm, nr, crt, 0, 0);
        g53c_dump_maxdiff(df, "last-layer routed of GPU nrm (GPU routed vs CPU ffn of GPU nrm, no shared)",
                          grt, crt, nd);
        if (g53c_dump_xin && g53c_dump_nr == nr) {
            float *cres = malloc((size_t)nr * HD * sizeof(float));
            float *cbr = malloc(nd * sizeof(float));
            float *cpost = malloc((size_t)nr * H * sizeof(float));
            float *ccomb = malloc((size_t)nr * H * H * sizeof(float));
            float *cnxt = malloc((size_t)nr * HD * sizeof(float));
            float *ccol = malloc(nd * sizeof(float));
            float *cnrm = malloc(nd * sizeof(float));
            float *csg = malloc((size_t)nr * wide * sizeof(float));
            float *csu = malloc((size_t)nr * wide * sizeof(float));
            int okp = cres && cbr && cpost && ccomb && cnxt && ccol && cnrm && csg && csu &&
                      g53c_dump_cpu_to_post(m, g53c_dump_xin, nr, g53c_dump_start, g53c_dump_lo,
                                            layer, 1, cres, cbr, cpost, ccomb,
                                            cnxt, ccol, cnrm, csg, csu);
            if (!okp) fprintf(df, "[dump] last-layer FFN of CPU replay nrm: not sampled\n");
            else {
                g53c_dump_maxdiff(df, "last-layer nrm into FFN (GPU nrm vs CPU replay nrm, at MoE)",
                                  gnrm, cnrm, nd);
                mlp3_rows(cds, cnrm, nr, &l->rg, &l->ru, &l->rd, c->swiglu_limit, sg, su);
                g53c_dump_maxdiff(df, "last-layer shared of CPU replay nrm (GPU ds vs CPU mlp3 of CPU nrm)",
                                  gds, cds, nd);
                ffn_layer_ex(m, l, layer, cnrm, nr, crt, 0, 0);
                g53c_dump_maxdiff(df, "last-layer routed of CPU replay nrm (GPU routed vs CPU ffn of CPU nrm, no shared)",
                                  grt, crt, nd);
                ffn_layer_ex(m, l, layer, cnrm, nr, cbr, 1, 0);
                {
                    float *gadd = malloc(nd * sizeof(float));
                    if (gadd) {
                        for (size_t k = 0; k < nd; k++) gadd[k] = gds[k] + grt[k];
                        g53c_dump_maxdiff(df, "last-layer FFN of CPU replay nrm (GPU shared+routed vs CPU ffn of CPU nrm)",
                                          gadd, cbr, nd);
                    }
                    free(gadd);
                }
                /* router top-k of GPU nrm vs CPU nrm */
                {
                    float *sa = malloc((size_t)E * sizeof(float)), *sb = malloc((size_t)E * sizeof(float));
                    int *pa = malloc((size_t)topk * sizeof(int)), *pb = malloc((size_t)topk * sizeof(int));
                    int nmis = 0, ncmp = 0;
                    double ds = 0, ms = 0;
                    if (sa && sb && pa && pb) {
                        for (int t = 0; t < nr; t++) {
                            const float *ra = gnrm + (size_t)t * D, *rb = cnrm + (size_t)t * D;
                            for (int e = 0; e < E; e++) {
                                const float *w = l->router + (size_t)e * D;
                                float suma = 0.f, sumb = 0.f;
                                for (int d0 = 0; d0 < D; d0++) { suma += w[d0] * ra[d0]; sumb += w[d0] * rb[d0]; }
                                sa[e] = sigmoidf_(suma); sb[e] = sigmoidf_(sumb);
                                double a = fabs((double)sa[e] - (double)sb[e]); if (a > ds) ds = a;
                                double b = fabs((double)sb[e]); if (b > ms) ms = b;
                            }
                            for (int k = 0; k < topk; k++) {
                                int ba = -1, bb = -1; float va = -INFINITY, vb = -INFINITY;
                                for (int e = 0; e < E; e++) {
                                    int ua = 0, ub = 0;
                                    for (int j = 0; j < k; j++) { if (pa[j] == e) ua = 1; if (pb[j] == e) ub = 1; }
                                    float ca = sa[e] + (l->rbias ? l->rbias[e] : 0.f);
                                    float cb = sb[e] + (l->rbias ? l->rbias[e] : 0.f);
                                    if (!ua && ca > va) { va = ca; ba = e; }
                                    if (!ub && cb > vb) { vb = cb; bb = e; }
                                }
                                pa[k] = ba; pb[k] = bb;
                            }
                            for (int k = 0; k < topk; k++, ncmp++) if (pa[k] != pb[k]) nmis++;
                        }
                        fprintf(df, "[dump] last-layer router scores (GPU nrm vs CPU nrm): max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                                ds, ms, ms > 0 ? ds / ms : 0, 1e-4 * ms);
                        fprintf(df, "[dump] last-layer router topk (GPU nrm vs CPU nrm): mismatches=%d of %d topk=%d experts=%d\n",
                                nmis, ncmp, topk, E);
                    } else fprintf(df, "[dump] last-layer router topk: not sampled\n");
                    free(sa); free(sb); free(pa); free(pb);
                }
            }
            free(cres); free(cbr); free(cpost); free(ccomb); free(cnxt);
            free(ccol); free(cnrm); free(csg); free(csu);
        }
        fflush(df);
    }
    free(ggs); free(gus); free(ghs); free(chs); free(cds); free(crt); free(sg); free(su);
}

/* The prompt block forward_prefill hands the chain: its chunk, when the chain runs with
 * a chunk from the budget (vkc_chunk_auto), the smaller of the two chains'; 0 =
 * forward_prefill's own (GLM53_PREFILL_CHUNK). */
static int g53c_prefill_rows(GModel *m, GSession *s, int n) {
    G53Chain *ch = g_g53c, *ch2 = g_g53c2;
    if (!g_vk_chain || !ch || !ch->ok || ch->failed || !vkc_chunk_auto()) return 0;
    if (!g53c_mirror(ch, m, s->filled + n, s->cap, 1)) return 0;
    int rows = g53c_rows(ch, m, s->cap);
    if (ch->ks.on && rows > ch->ks.chunk) rows = ch->ks.chunk;
    if (ch2 && ch2->ok && !ch2->failed) {
        vkc_device(1);
        if (g53c_mirror(ch2, m, s->filled + n, s->cap, 1)) {
            int r2 = g53c_rows(ch2, m, s->cap);
            if (ch2->ks.on && r2 > ch2->ks.chunk) r2 = ch2->ks.chunk;
            if (r2 < rows) rows = r2;
        }
        vkc_device(0);
    }
    return rows;
}
/* One chain's layers (lo..lo+n-1, on its device, current) for n rows of `streams` (the
 * session's positions start..), the streams after the last of them back into it. Returns
 * ch->n; 0 = not taken: memory refused (the streams untouched), or *lost = 1: a frame
 * failed (the device marked lost). */
static int g53c_forward_seg(GModel *m, GSession *s, G53Chain *ch, float *streams, int n, int start, int *lost) {
    *lost = 0;
    const Cfg *c = &m->c; int D = c->hidden, H = c->hc_mult, HD = H * D, lo = ch->lo, hi = lo + ch->n;
    int mirror_ok = g53c_mirror(ch, m, start + n, s->cap, 1);
    int CH = mirror_ok ? g53c_rows(ch, m, s->cap) : 1, rows = n < CH ? n : CH;
    if (ch->ks.on && rows > ch->ks.chunk) rows = ch->ks.chunk;   /* a step's rows fit the split's window */
    if (!mirror_ok || !g53c_scratch(ch, m, rows, start + n)) {
        /* COLI_VK_CHAIN_FAULT can land on a scratch zero/upload: that is a lost
         * frame (the CPU takes the forward), not an OOM decline. Count it so the
         * report's N forwards is > 0, as inkling/olmoe do. */
        if (vkc_lost()) { *lost = 1; ch->forwards++; return 0; }
        fprintf(stderr, "[VK] %s chain: device memory for %d rows at %d positions refused; per-matrix path\n",
                g53c_name(ch), rows, start + n);
        return 0;
    }
    vkc_gemm_rows(-1);
    /* the final streams wait here until every chunk is through: a lost device leaves
     * the caller's rows the forward's input, for the CPU to run again */
    float *outs = malloc((size_t)n * HD * sizeof(float));
    if (!outs) return 0;
    FILE *df_res = (n > 1) ? g53c_dump_fp() : NULL;
    float *xin0 = NULL, *gxs_d = NULL, *xs_d = NULL, *nxt_d = NULL, *col_d = NULL, *post_d = NULL, *comb_d = NULL, *nrm_d = NULL, *br_d = NULL, *sg_d = NULL, *su_d = NULL;
    int wide_d = c->dense_inter > c->moe_inter ? c->dense_inter : c->moe_inter;
    if (df_res) {
        xin0 = malloc((size_t)n * HD * sizeof(float));
        gxs_d = malloc((size_t)n * HD * sizeof(float));
        xs_d = malloc((size_t)n * HD * sizeof(float));
        nxt_d = malloc((size_t)n * HD * sizeof(float));
        col_d = malloc((size_t)n * D * sizeof(float));
        post_d = malloc((size_t)n * H * sizeof(float));
        comb_d = malloc((size_t)n * H * H * sizeof(float));
        nrm_d = malloc((size_t)n * D * sizeof(float));
        br_d = malloc((size_t)n * D * sizeof(float));
        sg_d = malloc((size_t)n * wide_d * sizeof(float));
        su_d = malloc((size_t)n * wide_d * sizeof(float));
        if (!(xin0 && gxs_d && xs_d && nxt_d && col_d && post_d && comb_d && nrm_d && br_d && sg_d && su_d)) {
            free(xin0); free(gxs_d); free(xs_d); free(nxt_d); free(col_d); free(post_d); free(comb_d);
            free(nrm_d); free(br_d); free(sg_d); free(su_d);
            xin0 = NULL; df_res = NULL;
        } else memcpy(xin0, streams, (size_t)n * HD * sizeof(float));
    }
    g53c_dump_xin = xin0;
    g53c_dump_nr = 0;
    g53c_dump_start = start;
    g53c_dump_lo = lo;
    for (int c0 = 0; c0 < n; c0 += rows) {
        int nr = n - c0 < rows ? n - c0 : rows, pb = start + c0;
        if (xin0) { g53c_dump_xin = xin0 + (size_t)c0 * HD; g53c_dump_nr = nr; g53c_dump_start = pb; g53c_dump_lo = lo; }
        if (!vkc_begin() || !g53c_push(ch, m, s, pb, nr) || !vkc_write(ch->xs, 0, streams + (size_t)c0 * HD, (size_t)nr * HD * sizeof(float)))
            goto lost;
        int ok = 1, pending = 0, pulled = lo;
        for (int i = lo; i < hi && ok; i++) {
            const GLayer *l = &m->layer[i];
            if (pending) {                       /* the FFN branch of the layer before: shared + routed, written back */
                FILE *dfm = (df_res && c0 == 0 && nr > 1) ? df_res : NULL;
                float *gnrm = NULL, *gds = NULL, *grt = NULL, *gbr = NULL, *cbr = NULL;
                if (dfm) {
                    gnrm = malloc((size_t)nr * D * 4); gds = malloc((size_t)nr * D * 4);
                    grt = malloc((size_t)nr * D * 4); gbr = malloc((size_t)nr * D * 4);
                    cbr = malloc((size_t)nr * D * 4);
                    int rd = gnrm && gds && grt && gbr && cbr &&
                             vkc_read(ch->h2d, 0, gnrm, (size_t)nr * D * 4) &&
                             vkc_read(ch->ds, 0, gds, (size_t)nr * D * 4) &&
                             vkc_read(ch->routed, 0, grt, (size_t)nr * D * 4);
                    if (!rd) fprintf(dfm, "[dump] last-layer moe add: not sampled\n");
                    else {
                        double ds = 0, ms = 0, dr = 0, mr = 0;
                        ffn_layer_ex((GModel *)m, &m->layer[i - 1], i - 1, gnrm, nr, cbr, 1, 0);
                        for (size_t k = 0; k < (size_t)nr * D; k++) {
                            double a = fabs((double)gds[k] + (double)grt[k] - (double)cbr[k]); if (a > ds) ds = a;
                            double b = fabs((double)cbr[k]); if (b > ms) ms = b;
                        }
                        fprintf(dfm, "[dump] last-layer moe add (GPU shared+routed vs CPU ffn of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                                nr, D, ds, ms, ms > 0 ? ds / ms : 0, 1e-4 * ms);
                        /* add-order: GPU is shared + (routed sum); CPU is ((shared + e0) + e1) */
                        double da = 0, ma = 0;
                        for (size_t k = 0; k < (size_t)nr * D; k++) {
                            float gadd = gds[k] + grt[k];
                            double a = fabs((double)gadd - ((double)gds[k] + (double)grt[k])); if (a > da) da = a;
                            double b = fabs((double)gadd); if (b > ma) ma = b;
                        }
                        fprintf(dfm, "[dump] last-layer moe add f32 (shared+routed) vs f64: max |diff| %.2e of %.2e rel=%.3e\n",
                                da, ma, ma > 0 ? da / ma : 0);
                        fflush(dfm);
                        g53c_dump_last_ffn_parts(ch, m, i - 1, nr, gnrm, gds, grt);
                    }
                    if (!vkc_begin()) ok = 0;
                }
                VkcEw add = {VKC_EW_ADD, nr * D, D, 1, 0, 1, 0, 0, 0, 0, 0, 1.f};
                ok = ok && vkc_ew(ch->br, ch->ds, ch->routed, NULL, NULL, &add);
                if (ok && dfm && gbr && vkc_read(ch->br, 0, gbr, (size_t)nr * D * 4) && gnrm && cbr) {
                    ffn_layer_ex((GModel *)m, &m->layer[i - 1], i - 1, gnrm, nr, cbr, 1, 0);
                    double d = 0, mc = 0;
                    for (size_t k = 0; k < (size_t)nr * D; k++) {
                        double a = fabs((double)gbr[k] - (double)cbr[k]); if (a > d) d = a;
                        double b = fabs((double)cbr[k]); if (b > mc) mc = b;
                    }
                    fprintf(dfm, "[dump] last-layer moe br after ADD (GPU br vs CPU ffn of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            nr, D, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
                    fflush(dfm);
                    if (!vkc_begin()) ok = 0;
                }
                free(gnrm); free(gds); free(grt); free(gbr); free(cbr);
                ok = ok && g53c_post(ch, m, nr);
                pending = 0;
            }
            double ta = now_s();
            ok = ok && g53c_pre(ch, m, ch->fna[i], ch->o_hca[i], ch->o_in[i], nr) &&
                 (c->is_full[i] ? g53c_mla(ch, m, s, l, i, nr, pb) : g53c_kda(ch, m, l, i, nr)) &&
                 g53c_post(ch, m, nr) &&
                 g53c_pre(ch, m, ch->fnf[i], ch->o_hcf[i], ch->o_post[i], nr);
            if (!ok) break;
            if (i < c->first_dense) {
                ok = g53c_mlp(ch, m, &l->dg, &l->du, &l->dd, ch->br, nr) && g53c_post(ch, m, nr);
                m->t_attn += now_s() - ta;
                if (ok && df_res && c0 == 0 && nr > 1 &&
                    vkc_read(ch->xs, 0, gxs_d, (size_t)nr * HD * 4) &&
                    g53c_dump_cpu_layers(m, xs_d, xin0 + (size_t)c0 * HD, nr, pb, lo, i,
                                         nxt_d, col_d, post_d, comb_d, nrm_d, br_d, sg_d, su_d)) {
                    double d = 0, mc = 0;
                    for (size_t k = 0; k < (size_t)nr * HD; k++) {
                        double a = fabs((double)gxs_d[k] - (double)xs_d[k]); if (a > d) d = a;
                        double b = fabs((double)xs_d[k]); if (b > mc) mc = b;
                    }
                    fprintf(df_res, "[dump] layer%d composed streams (GPU xs vs CPU %d layer of input): S=%d HD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            i, i - lo + 1, nr, HD, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
                    {
                        char tag[80];
                        snprintf(tag, sizeof tag, "layer%d composed", i);
                        g53c_dump_stream_rms_mean(df_res, tag, nr, H, D, c->eps, gxs_d, xs_d);
                    }
                    fflush(df_res);
                    if (!vkc_begin()) ok = 0;
                } else if (df_res && c0 == 0 && nr > 1)
                    fprintf(df_res, "[dump] layer%d composed streams: not sampled\n", i);
                continue;
            }
            /* A1; while it runs, the tier loads the experts this layer will likely
             * stream (a big prompt chunk only). Last model layer: host FFN so br
             * matches the CPU kernels (matmul_q / matmul_i4); chain forwards stay. */
            ok = vkc_copy(ch->h2d, 0, ch->nrm, 0, (size_t)nr * D) && vkc_submit(0);
            if (ok && i + 1 != c->n_layers) vkt_stream_prefetch(i, nr);
            ok = ok && vkc_finish();
            m->t_attn += now_s() - ta;
            if (!ok) break;
            g53c_pull(ch, m, s, pulled, i + 1, pb, nr); pulled = i + 1;
            if (i + 1 == c->n_layers) {
                double t1 = now_s();
                ffn_layer_ex(m, l, i, (const float *)vkc_ptr(ch->h2d), nr, ch->host_routed, 1, 1);
                double dt = now_s() - t1; ch->host_ms += dt * 1e3; m->t_ffn += dt;
                ok = vkc_begin() && vkc_write(ch->br, 0, ch->host_routed, (size_t)nr * D * sizeof(float)) &&
                     g53c_post(ch, m, nr);
                pending = 0;
                continue;
            }
            /* A2: the shared expert, while the host computes the routed experts */
            ok = vkc_begin() && g53c_mlp(ch, m, &l->rg, &l->ru, &l->rd, ch->ds, nr) && vkc_submit(0);
            double t1 = now_s();
            ffn_layer_ex(m, l, i, (const float *)vkc_ptr(ch->h2d), nr, ch->host_routed, 0, 0);
            memcpy(vkc_ptr(ch->routed), ch->host_routed, (size_t)nr * D * sizeof(float));
            double dt = now_s() - t1; ch->host_ms += dt * 1e3; m->t_ffn += dt;
            ok = ok && vkc_begin();
            pending = 1;
        }
        if (ok && pending) {
            FILE *dfm = (df_res && c0 == 0 && nr > 1) ? df_res : NULL;
            float *gnrm = NULL, *gds = NULL, *grt = NULL, *gbr = NULL, *cbr = NULL;
            if (dfm) {
                gnrm = malloc((size_t)nr * D * 4); gds = malloc((size_t)nr * D * 4);
                grt = malloc((size_t)nr * D * 4); gbr = malloc((size_t)nr * D * 4);
                cbr = malloc((size_t)nr * D * 4);
                int rd = gnrm && gds && grt && gbr && cbr &&
                         vkc_read(ch->h2d, 0, gnrm, (size_t)nr * D * 4) &&
                         vkc_read(ch->ds, 0, gds, (size_t)nr * D * 4) &&
                         vkc_read(ch->routed, 0, grt, (size_t)nr * D * 4);
                if (!rd) fprintf(dfm, "[dump] last-layer moe add: not sampled\n");
                else {
                    ffn_layer_ex((GModel *)m, &m->layer[hi - 1], hi - 1, gnrm, nr, cbr, 1, 0);
                    double ds = 0, ms = 0;
                    for (size_t k = 0; k < (size_t)nr * D; k++) {
                        float gadd = gds[k] + grt[k];
                        double a = fabs((double)gadd - (double)cbr[k]); if (a > ds) ds = a;
                        double b = fabs((double)cbr[k]); if (b > ms) ms = b;
                    }
                    fprintf(dfm, "[dump] last-layer moe add (GPU shared+routed vs CPU ffn of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                            nr, D, ds, ms, ms > 0 ? ds / ms : 0, 1e-4 * ms);
                    fflush(dfm);
                    g53c_dump_last_ffn_parts(ch, m, hi - 1, nr, gnrm, gds, grt);
                }
                if (!vkc_begin()) ok = 0;
            }
            VkcEw add = {VKC_EW_ADD, nr * D, D, 1, 0, 1, 0, 0, 0, 0, 0, 1.f};
            ok = ok && vkc_ew(ch->br, ch->ds, ch->routed, NULL, NULL, &add);
            if (ok && dfm && gbr && vkc_read(ch->br, 0, gbr, (size_t)nr * D * 4) && gnrm && cbr) {
                ffn_layer_ex((GModel *)m, &m->layer[hi - 1], hi - 1, gnrm, nr, cbr, 1, 0);
                double d = 0, mc = 0;
                for (size_t k = 0; k < (size_t)nr * D; k++) {
                    double a = fabs((double)gbr[k] - (double)cbr[k]); if (a > d) d = a;
                    double b = fabs((double)cbr[k]); if (b > mc) mc = b;
                }
                fprintf(dfm, "[dump] last-layer moe br after ADD (GPU br vs CPU ffn of GPU nrm): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                        nr, D, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
                fflush(dfm);
                if (!vkc_begin()) ok = 0;
            }
            free(gnrm); free(gds); free(grt); free(gbr); free(cbr);
            ok = ok && g53c_post(ch, m, nr);
        }
        if (ok && df_res && c0 == 0 && nr > 1 &&
            vkc_read(ch->xs, 0, gxs_d, (size_t)nr * HD * 4) &&
            g53c_dump_cpu_layers(m, xs_d, xin0 + (size_t)c0 * HD, nr, pb, lo, hi - 1,
                                 nxt_d, col_d, post_d, comb_d, nrm_d, br_d, sg_d, su_d)) {
            double d = 0, mc = 0;
            for (size_t k = 0; k < (size_t)nr * HD; k++) {
                double a = fabs((double)gxs_d[k] - (double)xs_d[k]); if (a > d) d = a;
                double b = fabs((double)xs_d[k]); if (b > mc) mc = b;
            }
            fprintf(df_res, "[dump] layer%d composed streams (GPU xs vs CPU %d layer of input): S=%d HD=%d max |diff| %.2e of %.2e rel=%.3e thresh_1e-4*m=%.2e\n",
                    hi - 1, hi - lo, nr, HD, d, mc, mc > 0 ? d / mc : 0, 1e-4 * mc);
            {
                char tag[80];
                snprintf(tag, sizeof tag, "layer%d composed", hi - 1);
                g53c_dump_stream_rms_mean(df_res, tag, nr, H, D, c->eps, gxs_d, xs_d);
            }
            /* host mean of GPU streams vs sequential CPU mean of those same GPU streams */
            double dm = 0, mm = 0;
            for (int t = 0; t < nr; t++) for (int d0 = 0; d0 < D; d0++) {
                float sum = 0.f; double dsum = 0;
                for (int h = 0; h < H; h++) {
                    float v = gxs_d[((size_t)t * H + h) * D + d0];
                    sum += v; dsum += (double)v;
                }
                float gmean = sum / H; float cmean = (float)(dsum / (double)H);
                double a = fabs((double)gmean - (double)cmean); if (a > dm) dm = a;
                double b = fabs((double)cmean); if (b > mm) mm = b;
            }
            fprintf(df_res, "[dump] residual mean of GPU streams (f32 sequential /H vs f64 /H): S=%d D=%d max |diff| %.2e of %.2e rel=%.3e\n",
                    nr, D, dm, mm, mm > 0 ? dm / mm : 0);
            fflush(df_res);
            if (!vkc_begin()) ok = 0;
        }
        ok = ok && vkc_copy(ch->xd, 0, ch->xs, 0, (size_t)nr * HD) && vkc_submit(1);
        if (!ok) goto lost;
        g53c_pull(ch, m, s, pulled, hi, pb, nr);
        memcpy(outs + (size_t)c0 * HD, vkc_ptr(ch->xd), (size_t)nr * HD * sizeof(float));
        for (int i = lo; i < hi; i++) if (c->is_full[i]) { ch->kv_valid[i] = pb + nr; vkc_kv_done(&ch->ks, ch->mla_ord[i], pb + nr); }
        ch->where = G53C_DEV;
    }
    memcpy(streams, outs, (size_t)n * HD * sizeof(float));
    free(outs);
    g53c_dump_xin = NULL; g53c_dump_nr = 0;
    free(xin0); free(gxs_d); free(xs_d); free(nxt_d); free(col_d); free(post_d); free(comb_d);
    free(nrm_d); free(br_d); free(sg_d); free(su_d);
    ch->forwards++;
    return ch->n;
lost:   /* a frame failed: the device is gone (or would not take a command) */
    free(outs);
    g53c_dump_xin = NULL; g53c_dump_nr = 0;
    free(xin0); free(gxs_d); free(xs_d); free(nxt_d); free(col_d); free(post_d); free(comb_d);
    free(nrm_d); free(br_d); free(sg_d); free(su_d);
    if (!vkc_lost()) { vkc_finish(); coli_vk_mark_lost_dev(ch->d); }
    *lost = 1;
    ch->forwards++;   /* this forward ran until the loss; g53c_report still prints N > 0 */
    return 0;
}
/* The devices' layers (every layer, or the first N of a partial chain, and the second
 * device's after them) for n rows of `streams` (the session's positions start..), the
 * streams after the last of them back into it. Returns how many layers ran: L, or fewer
 * (run_layers goes on from there on the CPU); 0 = not taken: the CPU runs the layers
 * (streams as they came in). */
static int g53c_forward(GModel *m, GSession *s, float *streams, int n, int start) {
    G53Chain *ch = g_g53c, *ch2 = g_g53c2;
    if (!g_vk_chain || !ch || !ch->ok || ch->failed) return 0;
    if (g_vk_chain == COLI_VK_CHAIN_PREFILL && n <= 2) return 0;
    if (ch2 && (!ch2->ok || ch2->failed || ch2->lo != ch->lo + ch->n)) ch2 = NULL;
    const Cfg *c = &m->c; int L = c->n_layers, D = c->hidden, H = c->hc_mult, HD = H * D;
    if (ch->owner != s) {                       /* another session takes the devices */
        if (ch->owner) g53c_sync_host(m, ch->owner);
        for (int d = 0; d < 2; d++) {
            G53Chain *x = g53c_of(d);
            if (!x || !x->ok) continue;
            x->owner = s; x->where = G53C_HOST;
            for (int i = 0; i < L; i++) x->kv_valid[i] = x->pool_valid[i] = 0;
            vkc_kv_reset(&x->ks);
        }
    }
    int dev0 = ch->where == G53C_DEV, dev2 = ch2 && ch2->where == G53C_DEV;
    int lost2 = 0;
    if (ch2) { vkc_device(1); lost2 = vkc_lost(); vkc_device(0); }
    if (vkc_lost() || lost2) { g53c_recover(m, s, start, dev0, dev2, lost2 ? ch2 : ch); return 0; }
    /* the input rows, for a rebuild should a device go: from the position where the
     * host's KDA state is current (here, when the devices take it up now) */
    if (ch->where == G53C_HOST) g53c_rec_reset(ch, start);
    else if (ch->rec_ok && start != ch->rec_base + ch->rec_len) {
        if (start < ch->rec_base + ch->rec_len && start >= ch->rec_base) ch->rec_len = start - ch->rec_base;
        else ch->rec_ok = 0;
    }
    if (ch->rec_ok) {
        if (ch->rec_len + n > ch->rec_cap) {
            int nc = ch->rec_cap ? ch->rec_cap : 256; while (nc < ch->rec_len + n) nc *= 2;
            float *r = realloc(ch->rec, (size_t)nc * D * sizeof(float));
            if (!r) ch->rec_ok = 0; else { ch->rec = r; ch->rec_cap = nc; }
        }
        if (ch->rec_ok) {
            for (int t = 0; t < n; t++) memcpy(ch->rec + (size_t)(ch->rec_len + t) * D, streams + (size_t)t * HD, (size_t)D * sizeof(float));
            ch->rec_len += n;
        }
    }
    /* the streams as they came in: the primary's replace them, and if the second device
     * is lost the CPU runs the forward again from these */
    float *keep = ch2 ? malloc((size_t)n * HD * sizeof(float)) : NULL;
    if (ch2 && !keep) ch2 = NULL;
    if (keep) memcpy(keep, streams, (size_t)n * HD * sizeof(float));
    int lost = 0;
    int done = g53c_forward_seg(m, s, ch, streams, n, start, &lost);
    if (!done) {
        free(keep);
        if (lost) { g53c_recover(m, s, start, dev0, dev2, ch); return 0; }
        g53c_cpu_step(m, s, start);   /* memory refused: the CPU from here on, the state current first */
        ch->failed = 1; g_vk_chain = 0;
        return 0;
    }
    if (!ch2) return done;
    vkc_device(1);
    int done2 = g53c_forward_seg(m, s, ch2, streams, n, start, &lost);
    vkc_device(0);
    if (lost) {
        memcpy(streams, keep, (size_t)n * HD * sizeof(float));
        free(keep);
        g53c_recover(m, s, start, dev0, dev2, ch2);
        return 0;
    }
    if (!done2) {   /* memory refused there: its layers on the CPU from this step on, their state current first */
        if (!g53c_sync_one(m, s, ch2)) {   /* lost on the way: the CPU runs the forward again, from its input */
            memcpy(streams, keep, (size_t)n * HD * sizeof(float));
            free(keep);
            g53c_recover(m, s, start, dev0, dev2, ch2);
            return 0;
        }
        free(keep);
        g53c_cpu_one(m, ch2, start);
        ch2->failed = 1;
        return done;
    }
    free(keep);
    return done + done2;
}

static void g53c_report(void) {
    for (int d = 0; d < 2; d++) {
        G53Chain *ch = g53c_of(d);
        if (!ch || !ch->ok || !ch->forwards) continue;
        int was = vkc_device(d);
        VkcStats st; vkc_stats(&st);
        fprintf(stderr, "[VK] %s chain: %llu forwards, %llu frames (%llu ops, %llu matmuls, %llu tiled GEMM), "
                        "%.1f ms waiting for the device, %.1f ms of routed experts on the host, %.1f MiB on the device\n",
                g53c_name(ch), ch->forwards, st.frames, st.ops, st.matmuls, st.gemms, st.wait_ms, ch->host_ms,
                st.dev_bytes / 1048576.0);
        vkc_kv_report(&ch->ks);
        vkc_prof_print();
        vkc_device(was);
    }
}

/* COLI_VK_CHAIN at load, once the device is open (model_load_range): before the expert
 * cache sizes itself from the free memory and before the tier (the trunk's device copies
 * count as used): the decision, the pipelines, the fit (how many layers the device holds),
 * COLI_VK_DENSE_HOST's decision for those layers (g53_dho_start), the tensors layer by
 * layer, then the head's host copy (g53_dho_finish). Without a fit (the chain off)
 * g53_dho_start does as before. */
static void g53c_start(GModel *m) {
    if (!g_vk_ready) return;
    const Cfg *c = &m->c;
    int tier_on = vkt_wanted() && m->streaming && c->n_experts > 0 && c->swiglu_limit > 0.f;
    if (!m->has_io || m->layer_begin != 0 || m->layer_end != c->n_layers) return;   /* a segment: no chain, no drop */
    int on = coli_vk_chain_decide("glm53", tier_on, COLI_VK_CHAIN_UNMEASURED);
    const char *no = NULL;
    const char *why = on ? g53c_check(m) : NULL;
    if (why) fprintf(stderr, "[VK] glm53 chain: %s; per-matrix path\n", why);
    /* the fit before anything of the chain is on the device (its pools' first blocks are
     * the fit's own granularity, as coli plan counts them) */
    if (on && !why) g53c_fit_plan(m);
    if (on && !(g_g53c_inited = vkc_init())) no = "the chain's pipelines did not come up";
    if (on && !no && !(vkc_mla_ready() && vkc_kda_ready() && vkc_mhc_ready()))
        no = "the MLA, KDA or mHC shaders are missing";
    if (no) fprintf(stderr, "[VK] glm53: %s: the dense chain stays off\n", no);
    if (no) g_g53c_fitted = 0;   /* no chain after all: everything as before */
    g53_dho_start(m, tier_on, g_g53c_fitted ? g_g53c_fit.n : -1, g_g53c_fitted ? g_g53c_fit.tail : 1);
    if (!g_g53c_fitted) return;
    int n0 = g_g53c_fit.n;
    if (g_g53c_fit.n > 0 && g53c_setup(m)) g_vk_chain = on;
    if (g_g53c_fit2_on) {
        vkc_device(1);
        if (!g_vk_chain || !g_g53c || g_g53c->n < n0) {
            /* the primary placed fewer layers than its fit: the second device's would not
             * follow them, so they stay on the CPU too */
            fprintf(stderr, "[VK] glm53 chain: the primary device stopped before layer %d; layers %d..%d stay on the CPU, "
                            "not on the second device\n", n0, n0, n0 + g_g53c_fit2.n - 1);
            g_g53c_fit2_on = 0;
            vkc_shutdown();
        } else {
            if (!g53c_setup_dev(m, 1)) { g_g53c_fit2_on = 0; vkc_shutdown(); }
            vkc_fit_placed("glm53 dev2", &g_g53c_fit2);
        }
        vkc_device(0);
    }
    g_g53_partial = vkc_fit_partial(&g_g53c_fit);
    for (int i = 0; i < g_g53c_fit.n; i++) g_g53c_lazy += g_g53c_mir[i];   /* for the tier (glm53_vk_tier_start) */
    vkc_fit_placed("glm53", &g_g53c_fit);
    g53_dho_finish(m, g_g53c_fit.n, g_g53c_fit.tail, g_g53_partial);
}
/* A partial chain at exit: the matrices the device holds, which must be the ones the
 * setup placed (the per-matrix path put nothing of a CPU layer or the head there). */
static void g53c_held_atexit(void) {
    size_t w = 0, nt = 0;
    coli_vk_mem_info(&w, &nt);
    fprintf(stderr, "[VK] glm53 chain: %d of %d layers held at exit: %zu B of matrices on the device\n",
            g_g53c_fit.n, g_g53c_fit.L, w);
}
/* After the tier's: at exit the chain goes before the device. */
static void g53c_atexit(void) {
    if (g_g53c_inited) atexit(vkc_shutdown_all);
    if (g_g53c_fitted && g_g53_partial) atexit(g53c_held_atexit);
}
