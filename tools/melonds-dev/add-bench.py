# Inserts a per-window retro_run timer into src/libretro/libretro.cpp (bench builds only).
import sys
p=sys.argv[1]+'/src/libretro/libretro.cpp'
s=open(p).read()
hook='''
#include <time.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdio.h>
static unsigned bench_frame;
static double bench_wall, bench_cpu;
static double bench_now(clockid_t c){ struct timespec t; clock_gettime(c,&t); return t.tv_sec*1e3+t.tv_nsec/1e6; }
static void bench_run(void);
void retro_run(void)
{
    double w0=bench_now(CLOCK_MONOTONIC), c0=bench_now(CLOCK_THREAD_CPUTIME_ID);
    bench_run();
    bench_frame++;
    if (bench_frame>300 && bench_frame<=2100) {
        bench_wall+=bench_now(CLOCK_MONOTONIC)-w0; bench_cpu+=bench_now(CLOCK_THREAD_CPUTIME_ID)-c0;
        if (bench_frame%120==60) {
            char b[96]; int n=snprintf(b,sizeof b,"%u %.3f %.3f\\n",bench_frame,bench_wall/120,bench_cpu/120);
            int fd=::open("/tmp/melon-bench.txt",O_WRONLY|O_CREAT|O_APPEND,0644); if(fd>=0){ (void)!::write(fd,b,n); ::close(fd);} 
            bench_wall=bench_cpu=0;
        }
    }
}
static void bench_run(void)
'''
assert s.count('\nvoid retro_run(void)\n')==1
s=s.replace('\nvoid retro_run(void)\n',hook,1)
old='\nbool retro_unserialize(const void *data, size_t size)\n{\n'
assert s.count(old)==1
s=s.replace(old, old+'   bench_frame = 0; bench_wall = bench_cpu = 0;\n',1)
open(p,'w').write(s)

# main-thread wait timers (bench builds only)
d=sys.argv[1]+'/src/'
def edit(f, old, new):
    s=open(d+f).read(); assert s.count(old)==1, (f, old[:50]); open(d+f,'w').write(s.replace(old,new,1))
edit('GPU.cpp','namespace GPU\n{\n','#include <time.h>\nnamespace GPU\n{\ndouble NuubWait[7];\ndouble NuubNow(){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec*1e3+t.tv_nsec/1e6; }\n')
edit('GPU.cpp','    Platform::Semaphore_Wait(Sema2DFree);\n','    { double t0=NuubNow(); Platform::Semaphore_Wait(Sema2DFree); NuubWait[0]+=NuubNow()-t0; }\n')
_g=open(d+'GPU.cpp').read()
_w='    Platform::Semaphore_Wait(Sema2DFence);\n'
_t='    { double t0=NuubNow(); Platform::Semaphore_Wait(Sema2DFence); NuubWait[1]+=NuubNow()-t0; }\n'
_g=_g.replace(_w+_w,_t+_t) if (_w+_w) in _g else _g.replace(_w,_t)
assert _t in _g
open(d+'GPU.cpp','w').write(_g)
edit('GPU3D_Soft.cpp','        Platform::Semaphore_Wait(Sema_RenderDone);\n    RenderIssued = false;','        { double t0=GPU::NuubNow(); Platform::Semaphore_Wait(Sema_RenderDone); GPU::NuubWait[2]+=GPU::NuubNow()-t0; }\n    RenderIssued = false;')
edit('GPU3D_Soft.cpp','        if (RenderThreadRendering)\n            Platform::Semaphore_Wait(Sema_RenderDone);','        if (RenderThreadRendering)\n            { double t0=GPU::NuubNow(); Platform::Semaphore_Wait(Sema_RenderDone); GPU::NuubWait[3]+=GPU::NuubNow()-t0; }')
s=open(d+'GPU3D_Soft.cpp').read()
i=s.index('namespace GPU3D')
s=s[:i]+'namespace GPU { extern double NuubWait[7]; double NuubNow(); }\n'+s[i:]
open(d+'GPU3D_Soft.cpp','w').write(s)
s=open(d+'libretro/libretro.cpp').read()
n=s.count('video_cb((uint8_t*)')
s=s.replace('video_cb((uint8_t*)','NUUB_VCB((uint8_t*)')
s=s.replace('#include "GPU.h"\n','#include "GPU.h"\nnamespace GPU { extern double NuubWait[7]; double NuubNow(); }\n#define NUUB_VCB(...) do { double t0_=GPU::NuubNow(); video_cb(__VA_ARGS__); GPU::NuubWait[4]+=GPU::NuubNow()-t0_; } while (0)\n',1)
s=s.replace('''"%u %.3f %.3f\\n",bench_frame,bench_wall/120,bench_cpu/120);''','''"%u %.3f %.3f w %.2f %.2f %.2f %.2f %.2f %.2f %.2f\\n",bench_frame,bench_wall/120,bench_cpu/120,GPU::NuubWait[0]/120,GPU::NuubWait[1]/120,GPU::NuubWait[2]/120,GPU::NuubWait[3]/120,GPU::NuubWait[4]/120,GPU::NuubWait[5]/120,GPU::NuubWait[6]/120);''',1)
s=s.replace('bench_wall=bench_cpu=0;\n','bench_wall=bench_cpu=0; for (int k=0;k<7;k++) GPU::NuubWait[k]=0;\n',1)
assert 'NuubWait[0]/120' in s and n>=3
open(d+'libretro/libretro.cpp','w').write(s)

edit('libretro/libretro.cpp','   audio_cb(buffer, size);\n','   { double t0_=GPU::NuubNow(); audio_cb(buffer, size); GPU::NuubWait[5]+=GPU::NuubNow()-t0_; }\n')
s=open(d+'libretro/input.cpp').read()
assert s.count('   input_poll_cb();\n')==1
s=s.replace('   input_poll_cb();\n','   { double t0_=GPU::NuubNow(); input_poll_cb(); GPU::NuubWait[6]+=GPU::NuubNow()-t0_; }\n',1)
s='namespace GPU { extern double NuubWait[7]; double NuubNow(); }\n'+s
open(d+'libretro/input.cpp','w').write(s)
