/* telemetry.h — dashboard protocol lines, stats/usage persistence, hardware probe.
 * Include after Model/Cfg/QT/ESlot/shards and st.h are defined; requires
 * qt_bytes(), now_s(), rss_gb(), edisk_s(), and the g_cuda_* globals (ifdef). */
#ifndef TELEMETRY_H
#define TELEMETRY_H

static int64_t tbytes(int O,int I,int bits){
    if(bits>=16) return (int64_t)O*I*4;
    if(bits>=5)  return (int64_t)O*I + (int64_t)O*4;
    return (int64_t)O*((I+1)/2) + (int64_t)O*4;
}

/* Container bytes of expert 0 of one layer (weights + .qs scales); 0 if absent. */
static int64_t expert_bytes_layer(Model *m, int layer){
    int64_t eb=0; char nm[256];
    const char *suf[3]={"gate_proj","up_proj","down_proj"};
    snprintf(nm,sizeof(nm),"model.layers.%d.mlp.experts.0.gate_proj.weight",layer);
    if(st_nbytes(&m->S,nm)<=0) return 0;
    for(int k=0;k<3;k++){
        snprintf(nm,sizeof(nm),"model.layers.%d.mlp.experts.0.%s.weight",layer,suf[k]);
        eb+=st_nbytes(&m->S,nm);
        snprintf(nm,sizeof(nm),"model.layers.%d.mlp.experts.0.%s.weight.qs",layer,suf[k]);
        int64_t q=st_nbytes(&m->S,nm); if(q>0) eb+=q;
    }
    return eb;
}

/* MAX over the widths the container actually holds, not the first MoE layer.
 * ws[] slots and the per-layer LRU slabs are reused ACROSS layers, and slab_cap
 * grows to the largest expert a slot has ever held and is never shrunk (see the
 * ESlot comment). So on a container that MIXES widths -- int4 routed layers with
 * an int8 MTP head, which is how GLM-5.2 ships -- every slot ends up costing the
 * widest one, while probing only c->first_dense reports the narrowest.
 *
 * cap_for_ram() divides by this, so the error goes straight into the cap. #766,
 * measured on 4x A6000 / 251 GiB:
 *
 *     layer 3  (int4 routed) 21.23 MB   <- what this used to return
 *     layer 78 (int8 MTP)    37.79 MB   <- what a slot actually costs
 *
 *   --auto-tier -> cap 113 -> 257 GB RSS -> OOM-kill
 *   113 * 21.23/37.79 = 63.5, and cap 64 is the measured point that survives at
 *   118 GB. The projection's shape was right; only this constant was wrong.
 *
 * nsp += 2 in cap_for_ram() already nods at the MTP row costing double, but that
 * corrects one row out of nsp; the slabs grow on all of them. */
static int64_t expert_bytes_probe(Model *m, int ebits){
    Cfg *c=&m->c;
    int64_t eb=expert_bytes_layer(m,c->first_dense);
    if(m->has_mtp){ int64_t mtp=expert_bytes_layer(m,c->n_layers); if(mtp>eb) eb=mtp; }
    if(eb<=0) eb = tbytes(c->moe_inter,c->hidden,ebits)*2 + tbytes(c->hidden,c->moe_inter,ebits);
    return eb;
}

/* BRAIN MAP: per-turn expert hit bitmap for the dashboard. */
static uint8_t **g_ehit;
static void ehit_mark(Model *m, int layer, int eid){
    if(!g_ehit){ Cfg *c=&m->c;
        g_ehit=calloc(c->n_layers+1,sizeof(uint8_t*));
        for(int i=0;i<=c->n_layers;i++) g_ehit[i]=calloc(c->n_experts,1);
    }
    g_ehit[layer][eid]=1;
}

/* CPU model + cores + RAM (GB); empty/zero where unavailable. */
#ifdef __APPLE__
#include <sys/sysctl.h>
#include <mach/mach.h>
#endif
static void hw_probe(char *cpu, size_t cn, int *cores, double *ram_total, double *ram_avail){
    cpu[0]=0;
#ifdef _WIN32
#if defined(__x86_64__) || defined(__i386__)
    { unsigned int r[12]={0}; unsigned int *w=r;
      for(unsigned int f=0x80000002u; f<=0x80000004u; f++,w+=4)
          __get_cpuid(f,&w[0],&w[1],&w[2],&w[3]);
      char *b=(char*)r; b[47]=0; while(*b==' ')b++;
      snprintf(cpu,cn,"%s",b); }
#endif
#elif defined(__APPLE__)
    { size_t sl=cn; if(sysctlbyname("machdep.cpu.brand_string",cpu,&sl,NULL,0)) cpu[0]=0; }
#else
    FILE *ci=fopen("/proc/cpuinfo","r");
    if(ci){ char ln[256];
        while(fgets(ln,sizeof(ln),ci)) if(!strncmp(ln,"model name",10)){
            char *p=strchr(ln,':'); if(p){ p++; while(*p==' ')p++;
            int n=(int)strlen(p); if(n>0&&p[n-1]=='\n')p[--n]=0;
            snprintf(cpu,cn,"%s",p); } break; }
        fclose(ci); }
#endif
    *cores=0;
#ifdef _WIN32
    { SYSTEM_INFO si; GetSystemInfo(&si); *cores=(int)si.dwNumberOfProcessors; }
#elif defined(_SC_NPROCESSORS_ONLN)
    *cores=(int)sysconf(_SC_NPROCESSORS_ONLN);
#endif
    *ram_total=*ram_avail=0;
#ifdef _WIN32
    compat_meminfo(ram_total,ram_avail);
#elif defined(__APPLE__)
    { uint64_t ms=0; size_t sl=sizeof(ms);
      if(!sysctlbyname("hw.memsize",&ms,&sl,NULL,0)) *ram_total=(double)ms/1e9;
      int64_t pgsz=0; sl=sizeof(pgsz);
      if(sysctlbyname("hw.pagesize",&pgsz,&sl,NULL,0)||pgsz<=0) pgsz=16384;
      vm_statistics64_data_t vs; mach_msg_type_number_t nc=HOST_VM_INFO64_COUNT;
      if(host_statistics64(mach_host_self(),HOST_VM_INFO64,(host_info64_t)&vs,&nc)==KERN_SUCCESS)
          /* macOS analogue of Linux MemAvailable: free + inactive + purgeable */
          *ram_avail=(double)(vs.free_count+vs.inactive_count+vs.purgeable_count)*(double)pgsz/1e9; }
#else
    FILE *mi=fopen("/proc/meminfo","r");
    if(mi){ char ln[256]; double mt=0,ma=0;
        while(fgets(ln,sizeof(ln),mi)){
            if(sscanf(ln,"MemTotal: %lf",&mt)==1) *ram_total=mt/1e6;
            if(sscanf(ln,"MemAvailable: %lf",&ma)==1) *ram_avail=ma/1e6;
        } fclose(mi); }
#endif
}

/* Shared grid geometry for EMAP / HITS: sparse MoE layers (+ MTP row when present)
 * × n_experts. Same rows/cols the process path prints. */
static void expert_grid_dims(Model *m, int *rows_out, int *cols_out){
    Cfg *c=&m->c;
    int rows=0;
    for(int i=0;i<c->n_layers;i++) if(m->L[i].sparse) rows++;
    int has_mtp = m->has_mtp && m->eusage && m->eusage[c->n_layers];
    if(has_mtp) rows++;
    if(rows_out) *rows_out=rows;
    if(cols_out) *cols_out=c->n_experts;
}

/* Fill helpers for SERVE printf and embed poll (same byte layouts).
 * emap/hits: cells/bits may be NULL to query size only (*out_len set).
 * Returns 0, or -2 if non-NULL buffer is too small for the needed length. */

static void hwinfo_fill(Model *m, int *cores, double *ram_total, double *ram_avail,
                        int *ngpu, double *vram_total, char *cpu, size_t cn,
                        char *gpu, size_t gn){
    Cfg *c=&m->c; (void)c;
    if(cpu && cn) cpu[0]=0;
    if(gpu && gn) gpu[0]=0;
    char cpu_local[256]; int c0=0; double rt=0,ra=0;
    hw_probe(cpu_local,sizeof(cpu_local),&c0,&rt,&ra);
    if(cores) *cores=c0;
    if(ram_total) *ram_total=rt;
    if(ram_avail) *ram_avail=ra;
    int ng=0; double vt=0;
    char gpu_name[128]="";
#ifdef COLI_CUDA
    ng=g_cuda_ndev; vt=m->gpu_expert_bytes/1e9;
    for(int i=0;i<g_cuda_ndev;i++){
        size_t fr=0,to=0; coli_cuda_mem_info(g_cuda_devices[i],&fr,&to);
        if(!i) vt=(double)to*g_cuda_ndev/1e9;
    }
    if(g_cuda_ndev>0)
        snprintf(gpu_name,sizeof(gpu_name),"CUDA device x%d",g_cuda_ndev);
#else
    (void)m;
#endif
    if(ngpu) *ngpu=ng;
    if(vram_total) *vram_total=vt;
    if(cpu && cn) snprintf(cpu,cn,"%s",cpu_local);
    if(gpu && gn) snprintf(gpu,gn,"%s",gpu_name);
}

static void tiers_fill(Model *m, int *vram_out, int *ram_out, int *disk_out,
                       double *vram_gb_out, double *ram_gb_out){
    Cfg *c=&m->c; int nsp=0;
    for(int i=0;i<c->n_layers;i++) if(m->L[i].sparse) nsp++;
    int total=(nsp+(m->has_mtp?1:0))*c->n_experts;
    int pinned=0,lru=0;
    for(int i=0;i<=c->n_layers;i++){ pinned+=m->npin?m->npin[i]:0; lru+=m->ecn?m->ecn[i]:0; }
    int vram=0; double vram_gb=0;
#ifdef COLI_CUDA
    vram=m->gpu_expert_count; vram_gb=m->gpu_expert_bytes/1e9;
#endif
    int ram=pinned-vram+lru; if(ram<0) ram=0;
    int disk=total-vram-ram; if(disk<0) disk=0;
    double eb=(double)expert_bytes_probe(m,m->ebits);
    if(vram_out) *vram_out=vram;
    if(ram_out) *ram_out=ram;
    if(disk_out) *disk_out=disk;
    if(vram_gb_out) *vram_gb_out=vram_gb;
    if(ram_gb_out) *ram_gb_out=ram*eb/1e9;
}

/* One byte per expert, row-major: byte = (tier<<6)|heat (tier 0 disk/1 RAM/2 VRAM). */
static int emap_fill(Model *m, uint8_t *cells, size_t cap, int *rows_out, int *cols_out,
                     size_t *out_len){
    Cfg *c=&m->c;
    int rows=0, cols=0;
    expert_grid_dims(m,&rows,&cols);
    size_t need=(size_t)rows*(size_t)cols;
    if(rows_out) *rows_out=rows;
    if(cols_out) *cols_out=cols;
    if(out_len) *out_len=need;
    if(!cells) return 0;
    if(cap<need) return -2;
    int has_mtp = m->has_mtp && m->eusage && m->eusage[c->n_layers];
    size_t w=0;
    for(int i=0;i<=c->n_layers;i++){
        int is_row = (i<c->n_layers && m->L[i].sparse) || (i==c->n_layers && has_mtp);
        if(!is_row) continue;
        for(int e=0;e<cols;e++){
            int tier=0;
            ESlot *P=m->pin?m->pin[i]:NULL;
            int np=m->npin?m->npin[i]:0;
            for(int z=0;z<np;z++) if(P[z].eid==e){
#ifdef COLI_CUDA
                tier = P[z].g.cuda?2:1;
#else
                tier = 1;
#endif
                break; }
            if(!tier && m->ecache && m->ecache[i])
                for(int z=0;z<m->ecn[i];z++) if(m->ecache[i][z].eid==e){ tier=1; break; }
            uint32_t u = m->eusage && m->eusage[i]?m->eusage[i][e]:0;
            int heat=0; while(u){ heat++; u>>=1; } if(heat>63) heat=63;
            cells[w++]=(uint8_t)((tier<<6)|heat);
        }
    }
    return 0;
}

/* 1 bit per expert (little-endian bit in byte), row-major. Clears g_ehit when
 * bits is non-NULL and the buffer is large enough (same as hits_emit). */
static int hits_fill(Model *m, uint8_t *bits, size_t cap, int *rows_out, int *cols_out,
                     size_t *out_len){
    Cfg *c=&m->c;
    int rows=0, cols=0;
    expert_grid_dims(m,&rows,&cols);
    size_t need=(size_t)((rows*cols+7)/8);
    if(rows_out) *rows_out=rows;
    if(cols_out) *cols_out=cols;
    if(out_len) *out_len=need;
    if(!bits) return 0;
    if(cap<need) return -2;
    memset(bits,0,need);
    if(!g_ehit) return 0;
    int has_mtp = m->has_mtp && m->eusage && m->eusage[c->n_layers];
    int bit=0;
    for(int i=0;i<=c->n_layers;i++){
        int is_row = (i<c->n_layers && m->L[i].sparse) || (i==c->n_layers && has_mtp);
        if(!is_row) continue;
        for(int e=0;e<cols;e++,bit++)
            if(g_ehit[i] && g_ehit[i][e]){ bits[bit>>3]|=(uint8_t)(1<<(bit&7)); g_ehit[i][e]=0; }
    }
    return 0;
}

static void hwinfo_emit(Model *m){
    char cpu[256], gpu_name[128];
    int cores=0, ngpu=0; double ram_total=0, ram_avail=0, vram_total=0;
    hwinfo_fill(m,&cores,&ram_total,&ram_avail,&ngpu,&vram_total,
                cpu,sizeof(cpu),gpu_name,sizeof(gpu_name));
    printf("HWINFO %d %.1f %.1f %d %.1f %s|%s\n",
        cores,ram_total,ram_avail,ngpu,vram_total,cpu,gpu_name);
    fflush(stdout);
}

static void tiers_emit(Model *m){
    int vram=0,ram=0,disk=0; double vram_gb=0,ram_gb=0;
    tiers_fill(m,&vram,&ram,&disk,&vram_gb,&ram_gb);
    printf("TIERS %d %d %d %.2f %.2f\n",vram,ram,disk,vram_gb,ram_gb);
    fflush(stdout);
}

static void emap_emit(Model *m){
    int rows=0, cols=0; size_t n=0;
    if(emap_fill(m,NULL,0,&rows,&cols,&n)!=0) return;
    uint8_t *cells=n?malloc(n):NULL;
    if(n && !cells) return;
    if(emap_fill(m,cells,n,&rows,&cols,&n)!=0){ free(cells); return; }
    char *hex=malloc(n*2+1); if(!hex){ free(cells); return; }
    int w=0;
    for(size_t b=0;b<n;b++){
        hex[w++]="0123456789abcdef"[cells[b]>>4];
        hex[w++]="0123456789abcdef"[cells[b]&15];
    }
    hex[w]=0;
    printf("EMAP %d %d %s\n",rows,cols,hex); fflush(stdout);
    free(hex); free(cells);
}

static void hits_emit(Model *m){
    if(!g_ehit) return;
    int rows=0, cols=0; size_t nb=0;
    if(hits_fill(m,NULL,0,&rows,&cols,&nb)!=0) return;
    uint8_t *bm=nb?calloc(nb,1):NULL;
    if(nb && !bm) return;
    if(hits_fill(m,bm,nb,&rows,&cols,&nb)!=0){ free(bm); return; }
    char *hex=malloc(nb*2+1); if(!hex){ free(bm); return; }
    int w=0;
    for(size_t b=0;b<nb;b++){
        hex[w++]="0123456789abcdef"[bm[b]>>4];
        hex[w++]="0123456789abcdef"[bm[b]&15];
    }
    hex[w]=0;
    printf("HITS %d %d %s\n",rows,cols,hex); fflush(stdout);
    free(hex); free(bm);
}

/* The history format lives in route_trace.h so every engine writes the same bytes;
 * these keep the Model-shaped call sites unchanged. */
static void stats_dump_q(Model *m, const char *path, int quiet){ (void)m; rt_save(path,quiet); }
static void stats_dump(Model *m, const char *path){ stats_dump_q(m,path,0); }

static char g_usage_path[2100]="";
static int64_t usage_load(Model *m, const char *path){ (void)m; return rt_load(path); }
static void usage_save(Model *m){ if(g_usage_path[0]) stats_dump_q(m,g_usage_path,1); }

#endif /* TELEMETRY_H */
