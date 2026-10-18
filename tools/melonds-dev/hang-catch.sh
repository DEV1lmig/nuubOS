#!/bin/sh
# hang-catch.sh VARIANT TRIES: launch/quit until RetroArch hangs on quit, dump stacks
V=$1; N=$2
nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; exit 1; }
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
rm -rf /tmp/hs; mkdir /tmp/hs; . /tmp/opts.sh
opt_set $C melonds_accuracy fast
mount --bind /tmp/melonds-$V.so /usr/lib/libretro/melonds_nuubos_libretro.so
i=1; while [ $i -le $N ]; do
  nuubos-emuctl launch f0e8c9e2dd6510f0 slot:1 >/dev/null; sleep 12
  P=$(pidof retroarch); cat /proc/$P/maps > /tmp/hs/maps
  nuubos-emuctl quit >/dev/null; sleep 5
  if [ -d /proc/$P/task ]; then
    for t in /proc/$P/task/*; do T=${t##*/}; sp=$(cut -d" " -f8 $t/syscall)
      echo "$T $(cat $t/comm) $sp $(cut -d' ' -f2 $t/syscall)" >> /tmp/hs/thr
      dd if=/proc/$P/mem of=/tmp/hs/stk-$T bs=4096 skip=$(( sp / 4096 )) count=4 2>/dev/null
      echo $(( sp / 4096 * 4096 )) > /tmp/hs/base-$T; done
    echo "hang on try $i"; break
  fi
  echo "try $i ok"; i=$((i+1)); sleep 3
done
for k in $(seq 1 30); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 2
umount /usr/lib/libretro/melonds_nuubos_libretro.so
opt_restore $C
cd /tmp/hs && tar cf /tmp/hs.tar .
