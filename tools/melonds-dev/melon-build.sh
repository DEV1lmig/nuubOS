#!/bin/sh
# usage: melon-build.sh VARIANT EXTRA_CXXFLAGS EXTRA_LDFLAGS VECT(0/1)
set -e
V=$1; X=$2; L=$3; VEC=$4
export PATH=/workspace/output/h700/host/bin:$PATH
rm -rf /scratch/mb-$V; cp -a ${SRC:-/scratch/melon-src} /scratch/mb-$V; cd /scratch/mb-$V
[ -n "$BENCH" ] && python3 /scratch/add-bench.py /scratch/mb-$V
[ "$VEC" = 1 ] && sed -i 's/-O3 -fno-tree-vectorize/-O3/' Makefile
B="-D_LARGEFILE_SOURCE -D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64 -O2 -g0"
CC=aarch64-buildroot-linux-gnu-gcc CXX=aarch64-buildroot-linux-gnu-g++ AR=aarch64-buildroot-linux-gnu-ar \
  CFLAGS="$B $X" CXXFLAGS="$B $X" LDFLAGS="-lm $L" make -f Makefile platform=unix ARCH=arm64 LD=aarch64-buildroot-linux-gnu-g++ -j8 > /scratch/mb-$V.log 2>&1
aarch64-buildroot-linux-gnu-strip -o /scratch/melonds-$V.so melonds_libretro.so
ls -la /scratch/melonds-$V.so
