/* test_ram_clamp.c -- #759 explicit RAM_GB vs physical memory contract.
 * coli_clamp_ram_gb() (colibri.c) must implement exactly: an explicit budget
 * larger than the memory actually available is clamped down to it, an explicit
 * budget that fits passes through untouched, the <=0 "auto" sentinel always
 * passes through, an unreadable snapshot (0) keeps the literal budget, and
 * COLI_RAM_OVERCOMMIT=1 restores take-it-literally behaviour for benchmarking.
 * It is a pure function (no I/O, no globals), so this exercises it directly
 * with fabricated inputs -- no model, no probe, no GPU hardware needed.
 * Portable: builds and runs on every platform, same as test_cap_precedence.c. */
#define main coli_glm_main_unused
#include "../colibri.c"
#undef main

static int failures = 0;

static void check(const char *label, double ram_gb, double avail, int oc, double want){
    double got = coli_clamp_ram_gb(ram_gb, avail, oc);
    if(fabs(got-want) > 1e-9){
        fprintf(stderr, "FAIL %-34s ram=%.1f avail=%.1f oc=%d -> %.1f (want %.1f)\n",
                label, ram_gb, avail, oc, got, want);
        failures++;
    }
}

int main(void){
    /* #759 repro shape: unified-memory host, #653 shrank the snapshot below the
     * user's explicit budget -- that budget must come down to what exists. */
    check("oversized budget clamps",        100.0, 59.0, 0, 59.0);
    check("oversized on discrete too",      200.0, 120.0, 0, 120.0);

    /* a budget that fits is honored literally */
    check("fitting budget untouched",       100.0, 128.0, 0, 100.0);
    check("exact fit untouched",             59.0, 59.0, 0, 59.0);

    /* auto sentinel and degenerate inputs pass through */
    check("auto sentinel untouched",         0.0, 59.0, 0, 0.0);
    check("negative sentinel untouched",    -1.0, 59.0, 0, -1.0);
    check("unreadable snapshot keeps literal", 100.0, 0.0, 0, 100.0);

    /* documented escape hatch: COLI_RAM_OVERCOMMIT=1 keeps the literal budget */
    check("overcommit keeps oversized",     100.0, 59.0, 1, 100.0);
    check("overcommit keeps fitting",       100.0, 128.0, 1, 100.0);

    /* Host-class budget: 64 GB default must not win on a ~90 GB APU. */
    { double got = coli_ram_budget_gb(64.0, 72.727, 90.0);
      if(fabs(got - 79.2) > 0.05){
          fprintf(stderr, "FAIL 64 default on 90GB host -> %.3f (want 79.2)\n", got);
          failures++;
      }
      got = coli_ram_budget_gb(0.0, 72.727, 90.0);
      if(fabs(got - 79.2) > 0.05){
          fprintf(stderr, "FAIL auto on 90GB host -> %.3f (want 79.2)\n", got);
          failures++;
      }
      got = coli_ram_budget_gb(32.0, 72.727, 90.0);
      if(fabs(got - 32.0) > 1e-9){
          fprintf(stderr, "FAIL operator 32 GB must win -> %.3f\n", got);
          failures++;
      }
    }

    /* cap_for_ram must not clamp host-class auto back to MemAvailable.
     * 88% of 90 GB is 79.2; MemAvailable 72.727 is the 64 GB default's input. */
    { double got = coli_resolve_ram_gb(64.0, 72.727, 90.0, 0, NULL);
      if(fabs(got - 79.2) > 0.05){
          fprintf(stderr, "FAIL resolve 64 default on 90GB host -> %.3f (want 79.2, not MemAvailable)\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(0.0, 72.727, 90.0, 0, NULL);
      if(fabs(got - 79.2) > 0.05){
          fprintf(stderr, "FAIL resolve auto on 90GB host -> %.3f (want 79.2)\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(79.2, 72.727, 90.0, 0, NULL);
      if(fabs(got - 79.2) > 0.05){
          fprintf(stderr, "FAIL resolve rust-written host class -> %.3f (want 79.2)\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(32.0, 72.727, 90.0, 0, NULL);
      if(fabs(got - 32.0) > 1e-9){
          fprintf(stderr, "FAIL resolve operator 32 GB must win -> %.3f\n", got);
          failures++;
      }
      /* dense-only glm: RAM_GB=3.7145 is just above the fixture reserve. Host
       * class (~79.2) and the auto 8 GB floor must not swallow that budget. */
      got = coli_resolve_ram_gb(3.7145, 72.727, 90.0, 0, NULL);
      if(fabs(got - 3.7145) > 1e-9){
          fprintf(stderr, "FAIL resolve operator 3.7145 GB must win -> %.3f (want 3.7145, not 8)\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(8.0, 72.727, 90.0, 0, NULL);
      if(fabs(got - 8.0) > 1e-9){
          fprintf(stderr, "FAIL resolve operator 8 GB must win -> %.3f\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(100.0, 59.0, 90.0, 0, NULL);
      if(fabs(got - 59.0) > 1e-9){
          fprintf(stderr, "FAIL resolve oversized explicit still clamps -> %.3f (want 59)\n", got);
          failures++;
      }
      got = coli_resolve_ram_gb(100.0, 59.0, 90.0, 1, NULL);
      if(fabs(got - 100.0) > 1e-9){
          fprintf(stderr, "FAIL resolve overcommit keeps oversized -> %.3f\n", got);
          failures++;
      }
    }

    /* Live strip: cap slots are RAM, not Disk 19456 / RAM 0. GPU 0 stays 0. */
    { int ram = coli_tiers_ram_experts(0, 0, 27*76, 19456);
      if(ram != 27*76){
          fprintf(stderr, "FAIL ram cap 27*76 -> %d (want %d)\n", ram, 27*76);
          failures++;
      }
      if(coli_tiers_ram_experts(10, 0, 5, 100) != 10){
          fprintf(stderr, "FAIL filled above cap must keep filled\n");
          failures++;
      }
      if(coli_tiers_ram_experts(0, 100, 50, 250) != 50){
          fprintf(stderr, "FAIL cpu-only cap must not steal GPU count into RAM wrongly\n");
          failures++;
      }
    }

    if(failures){ fprintf(stderr, "%d failure(s)\n", failures); return 1; }
    printf("test_ram_clamp: ok\n");
    return 0;
}
