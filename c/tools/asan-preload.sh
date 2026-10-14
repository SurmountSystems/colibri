#!/bin/sh
# Source this file. Do not execute it.
# Prepend GCC's ASan and UBSan runtimes to LD_PRELOAD so they are the
# first libraries loaded. A library already on LD_PRELOAD (libgomp, for
# example) makes an instrumented binary abort before main. A missing
# runtime leaves LD_PRELOAD unchanged. Compile steps must not source
# this: the flags stay -fsanitize=address,undefined on the compiler
# command, and this only wraps the run.
asan_so=$(gcc -print-file-name=libasan.so 2>/dev/null || true)
ubsan_so=$(gcc -print-file-name=libubsan.so 2>/dev/null || true)
if [ -n "$asan_so" ] && [ -f "$asan_so" ] && [ -n "$ubsan_so" ] && [ -f "$ubsan_so" ]; then
  LD_PRELOAD="${asan_so}:${ubsan_so}${LD_PRELOAD:+:$LD_PRELOAD}"
  export LD_PRELOAD
fi
unset asan_so ubsan_so
