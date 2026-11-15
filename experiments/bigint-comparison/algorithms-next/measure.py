#!/usr/bin/env python3
"""Run identical public-API batches and allocation-free forward kernels.

Pass optimized baseline/candidate/Malachite executables and an output directory.
Raw perf stdout/stderr, inputs, counters and alternating CPU observations survive.
"""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess


def run(args):
    return subprocess.run([str(x) for x in args], check=True, capture_output=True, text=True)


def main():
    p = argparse.ArgumentParser()
    for name in ('baseline', 'candidate', 'malachite', 'inputs', 'out'):
        p.add_argument('--' + name.replace('_', '-'), required=True, type=Path)
    p.add_argument('--kernel-c', type=Path)
    p.add_argument('--kernel-plenty', type=Path)
    p.add_argument('--compiler', type=Path)
    p.add_argument('--quick', action='store_true')
    p.add_argument('--pilot', action='store_true')
    p.add_argument('--kernels-only', action='store_true')
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    cpu = min(os.sched_getaffinity(0))
    bins = {name: getattr(a, name).resolve() for name in ('baseline', 'candidate', 'malachite')}
    inputs = json.loads(a.inputs.read_text())
    work = []
    for x in inputs:
        for op in ('add', 'mul', 'div', 'parse', 'render'):
            work.append((str(x['bits']), op, x['product'] if op == 'div' else x['a'], x['b']))
    rng = random.Random(987141)
    shapes = [(104,104), (105,105), (127,127), (128,128), (129,129), (191,128), (192,129), (193,130), (255,255), (256,256), (257,257), (511,511), (512,512), (159,128), (160,128), (161,128), (320,256), (321,256), (640,512), (641,512), (1023,1023), (1024,1024), (1025,1025), (1535,1535), (1536,1536), (1537,1537), (1791,1791), (1792,1792), (1793,1793), (1200,2384), (2048,2048), (2049,2049), (1024,2048), (2048,1024), (1024,2049), (2049,1024), (128,2)]
    for m,n in shapes:
        x = rng.getrandbits(m*64) | (1 << (m*64-1))
        y = rng.getrandbits(n*64) | (1 << (n*64-1))
        work.append((f'{m}x{n}limbs', 'mul', f'{x:x}', f'{y:x}'))
    if a.pilot:
        selected = {'256','2048','8192','65536','1025x1025limbs','1791x1791limbs','2048x2048limbs','1024x2049limbs','2049x1024limbs','128x2limbs'}
        work = [w for w in work if w[0] in selected and w[1] == 'mul']
    (a.out/'workloads.json').write_text(json.dumps(work)+'\n')
    if a.kernels_only:
        assert (a.out/'instructions.csv').is_file() and (a.out/'cpu.csv').is_file()
        work=[]
    rows, times = [], []
    for shape,op,x,y in work:
        pilot = run([bins['baseline'],op,x,y,1])
        ns = int(pilot.stdout.split()[0])
        count = max(2,min(100000,40000000//max(1,ns)))
        for repeat in range(3):
            checks = []
            for name in bins:
                counters = []
                for factor in (1,2):
                    tag = f'{shape}-{op}-{name}-{repeat}-{factor}'
                    r = run(['perf','stat','-x,','-e','instructions:u','--','taskset','-c',cpu,bins[name],op,x,y,count*factor])
                    (a.out/(tag+'.stdout')).write_text(r.stdout)
                    (a.out/(tag+'.stderr')).write_text(r.stderr)
                    event = next(z for z in csv.reader(r.stderr.splitlines()) if len(z)>2 and z[2]=='instructions:u')
                    counters.append(int(event[0]));checks.append((factor,int(r.stdout.split()[1])))
                rows.append((shape,op,name,repeat,count,*counters,(counters[1]-counters[0])/count))
            for factor in (1,2):
                assert len({check for f,check in checks if f==factor})==1
        if not a.quick:
            for repeat in range(5):
                names = list(bins) if repeat%2==0 else list(reversed(bins))
                for name in names:
                    r = run(['taskset','-c',cpu,bins[name],op,x,y,count])
                    ns,check = map(int,r.stdout.split())
                    times.append((shape,op,name,repeat,count,ns,ns/count,check))
        print('measured',shape,op,flush=True)
    if not a.kernels_only:
        with (a.out/'instructions.csv').open('w') as f:
            w=csv.writer(f);w.writerow(['shape','operation','engine','repeat','count','instructions_n','instructions_2n','instructions_per_op']);w.writerows(rows)
        with (a.out/'cpu.csv').open('w') as f:
            w=csv.writer(f);w.writerow(['shape','operation','engine','repeat','count','ns','ns_per_op','checksum']);w.writerows(times)
    if bool(a.kernel_c) != bool(a.kernel_plenty):
        raise ValueError('provide both kernel binaries')
    if a.kernel_c:
        kernels=[]
        checks={}
        for name,binary in [('c',a.kernel_c),('plenty',a.kernel_plenty)]:
            for repeat in range(3):
                counts=[]
                for n in (16,32):
                    r=run(['perf','stat','-x,','-e','instructions:u','--','taskset','-c',cpu,binary,n])
                    (a.out/f'kernel-{name}-{repeat}-{n}.stdout').write_text(r.stdout)
                    (a.out/f'kernel-{name}-{repeat}-{n}.stderr').write_text(r.stderr)
                    if n in checks:
                        assert checks[n]==r.stdout
                    checks[n]=r.stdout
                    counts.append(int(next(z for z in csv.reader(r.stderr.splitlines()) if len(z)>2 and z[2]=='instructions:u')[0]))
                kernels.append([name,repeat,*counts,(counts[1]-counts[0])/16])
        (a.out/'kernel.json').write_text(json.dumps(kernels,indent=2)+'\n')
    meta={'cpu_affinity':cpu,'binaries':{name:{'path':str(v),'sha256':hashlib.sha256(v.read_bytes()).hexdigest()} for name,v in bins.items()},'rustc':run(['rustc','--version']).stdout,'host':run(['uname','-a']).stdout}
    if a.compiler:
        meta['compiler_sha256']=hashlib.sha256(a.compiler.read_bytes()).hexdigest()
    source=Path(__file__).resolve().parents[3]/'stdlib/std/bigint.plenty'
    meta['bigint_source_sha256']=hashlib.sha256(source.read_bytes()).hexdigest()
    meta['gcc']=run(['gcc','--version']).stdout.splitlines()[0]
    meta['program_profile']='optimized default; no runtime-checks; scalar exact integer transforms'
    (a.out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')

if __name__=='__main__':
    main()
