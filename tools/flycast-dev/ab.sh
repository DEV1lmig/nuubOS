#!/bin/sh
# ab.sh ROUNDS SECONDS GAME:MODE A.so B.so: alternates two Flycast-nuubOS
# builds on the same state and prints, per run, the emulation thread's CPU
# milliseconds per emulated frame (the busiest thread's CPU time / frames
# counted by GET_FPS) and the mean fps. Puts A back at the end.
N=$1; T=$2; X=$3; A=$4; B=$5; L=/usr/lib/libretro/flycast_nuubos_libretro.so
G=${X%%:*}; M=${X#*:}
U=/userdata/users/$(cat /run/nuubos/user/active)/; S=$U/appdata/nuubos-emulation/settings.conf
cp $S /tmp/ab-settings.conf
nuubos-emuctl set overlay 1 >/dev/null; nuubos-emuctl set overlay_items "" >/dev/null
i=0
while [ $i -lt $N ]; do
  for so in $A $B; do
    nuubos-emuctl status | grep -q "^state=idle" || { echo BUSY; cp /tmp/ab-settings.conf $S; exit 1; }
    cp $so $L
    nuubos-emuctl launch $G $M >/dev/null; sleep 5
    P=$(pidof retroarch)
    t0=$(for t in /proc/$P/task/*; do cut -d' ' -f14,15 $t/stat | awk '{print $1+$2}'; done | sort -n | tail -1)
    f=0; k=0; while [ $k -lt $T ]; do sleep 1; v=$(nuubos-emuctl status | grep '^fps=' | cut -d= -f2); f=$(echo "$f $v" | awk '{print $1+$2}'); k=$((k+1)); done
    t1=$(for t in /proc/$P/task/*; do cut -d' ' -f14,15 $t/stat | awk '{print $1+$2}'; done | sort -n | tail -1)
    echo "$so $(echo "$t0 $t1 $f $T" | awk '{printf "emu %.2f ms/frame, %.1f fps", ($2-$1)*10/$3, $3/$4}')"
    kill -9 $P; for w in $(seq 1 20); do nuubos-emuctl status | grep -q "^state=idle" && break; sleep 1; done; sleep 1
  done
  i=$((i+1))
done
cp $A $L; cp /tmp/ab-settings.conf $S
