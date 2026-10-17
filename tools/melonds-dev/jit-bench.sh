#!/bin/sh
# jit-bench.sh GAME SLOT ROUNDS VARIANT...  (bench builds: /tmp/melonds-V.so)
G=$1; SL=$2; N=$3; shift 3
U=$(ls -d /userdata/users/*/ | head -1); C=$U/appdata/retroarch/retroarch-core-options.cfg
rm -rf /tmp/jb-states; cp -a $U/states/nds /tmp/jb-states; cp $C /tmp/jb-opts.cfg
sed -i 's/^melonds_frameskip = .*/melonds_frameskip = "disabled"/' $C
sed -i '/^melonds_accuracy = /d' $C; echo 'melonds_accuracy = "fast"' >> $C
LIB=/usr/lib/libretro/melonds_nuubos_libretro.so
r=1; while [ $r -le $N ]; do
  for x in "$@"; do
    v=${x%+*}; m=${x#*+}; [ "$m" = "$x" ] && m=none
    rm -f /tmp/melon-bench.txt
    mount --bind /tmp/melonds-$v.so $LIB
    nuubos-emuctl launch $G slot:$SL >/dev/null
    t=0; while [ $t -lt 40 ] && ! nuubos-emuctl status | grep -q "^state=running"; do sleep 1; t=$((t+1)); done
    sleep 1; nuubos-emuctl fast-forward >/dev/null
    case $m in iso*)
      t=0; while [ $t -lt 40 ] && [ "$(ls /proc/$(pidof retroarch)/task 2>/dev/null | wc -l)" -lt 8 ]; do sleep 1; t=$((t+1)); done
      sleep 1; P=$(pidof retroarch)
      /tmp/setaff 7 $(ls /proc/$P/task | grep -vx $P); /tmp/setaff 8 $P ;;
    pri*)
      t=0; while [ $t -lt 40 ] && [ "$(ls /proc/$(pidof retroarch)/task 2>/dev/null | wc -l)" -lt 8 ]; do sleep 1; t=$((t+1)); done
      sleep 1; P=$(pidof retroarch); renice -n -${m#pri} -p $P >/dev/null ;;
    esac
    t=0; fq=0; nf=0; while [ $t -lt 120 ] && [ "$(wc -l < /tmp/melon-bench.txt 2>/dev/null)" != 15 ]; do sleep 2; t=$((t+2)); fq=$((fq+$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)/1000)); nf=$((nf+1)); done
    nuubos-emuctl quit >/dev/null
    for i in $(seq 1 25); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done
    sleep 2; umount $LIB
    echo "$r $x mhz $((fq/(nf+1))) $(awk '{w+=$2; c+=$3} END {printf "wall %.3f cpu %.3f n %d", w/NR, c/NR, NR}' /tmp/melon-bench.txt) | $(awk '{printf "%s ", $2}' /tmp/melon-bench.txt)"
    cp /tmp/melon-bench.txt /tmp/raw-$r-$x.txt
    rm -rf $U/states/nds; cp -a /tmp/jb-states $U/states/nds
  done
  r=$((r+1))
done
cp /tmp/jb-opts.cfg $C
