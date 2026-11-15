#!/usr/bin/env python3
"""Source-derived proof of the locked Malachite 0.14 dispatcher/profile.

Only reads sources; no instrumentation of the measured dependency. Constants
and the full profile search are read directly from the verified crate sources.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

p=argparse.ArgumentParser();p.add_argument('source',type=Path);a=p.parse_args()
s=(a.source/'src/platform_64.rs').read_text()
constants={k:int(v) for k,v in re.findall(r'const (MUL_\w+_THRESHOLD): usize = (\d+)',s)}
m=(a.source/'src/natural/arithmetic/mul/mod.rs').read_text()
fft=int(re.search(r'FFT_MUL_THRESHOLD: usize = (\d+)',m)[1])
c=(a.source/'src/natural/arithmetic/mul/context.rs').read_text()
profiles=[tuple(map(int,x)) for x in re.findall(r'ProfileEntry \{\s*np: (\d+),\s*bits: (\d+),\s*bn_bound: (\d+)',c) if int(x[0])]
# The production search retains the largest eligible bits in each np band,
# then minimizes the truncation/depth cost, preserving first-on-tie order.
best=[]
for np in sorted({x[0] for x in profiles}):
    entries=[x for x in profiles if x[0]==np and 1024<=x[2]]
    np,bits,bound=entries[-1]
    alen=(1024*64+bits-1)//bits;zlen=2*alen-1
    trunc=(zlen+255)//256*256;depth=max(8,(trunc-1).bit_length())
    score=(1-.25*trunc/(1<<depth))*1e-6*np*depth*trunc
    best.append(dict(np=np,bits=bits,alen=alen,zlen=zlen,ztrunc=trunc,depth=depth,score=score))
print(json.dumps({'constants':constants,'equal_fft_threshold':fft,'profile_candidates_1024_limbs':best,'selected_profile':min(best,key=lambda x:x['score']),'path_evidence':{'16':'Small u64 limb multiply','256':'4 limbs <16: basecase','2048':'32 limbs in [16,105): Toom22','8192':'128 limbs in [105,300): Toom33','65536':'1024 limbs >=1000: FFT'},'source_hashes':{str(p.relative_to(a.source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [a.source/'src/platform_64.rs',a.source/'src/natural/arithmetic/mul/mod.rs',a.source/'src/natural/arithmetic/mul/context.rs',a.source/'src/natural/arithmetic/mul/fft/mod.rs',a.source/'src/natural/arithmetic/mul/fft/body.rs']}},indent=2))
