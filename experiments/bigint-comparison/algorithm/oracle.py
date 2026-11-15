#!/usr/bin/env python3
"""Independent Python and Malachite exact-products at transform boundaries."""
import argparse
import hashlib
import json
from pathlib import Path
import random
import subprocess

p=argparse.ArgumentParser()
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--malachite',type=Path,required=True)
p.add_argument('--out',type=Path,required=True)
a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
r=random.Random(987140)
cases=[]
for m,n in [(1023,1023),(1024,1024),(1025,1025),(1535,1535),(1536,1536),(1537,1537), (1791,1791), (1792,1792), (1793,1793), (1200,2384),(2048,2048),(2049,2049),(1024,2048),(2048,1024),(1024,2049),(2049,1024),(256,1024),(1024,256)]:
    for pattern in range(3):
        x = (1 << (m*64))-1 if pattern==0 else (1 << (m*64-1)) if pattern==1 else r.getrandbits(m*64)|(1 << (m*64-1))
        y = (1 << (n*64))-1 if pattern==0 else (1 << (n*64-1)) if pattern==1 else r.getrandbits(n*64)|(1 << (n*64-1))
        for sx,sy in [(1,1),(-1,1),(1,-1),(-1,-1)]: cases.append((sx*x,sy*y))
corpus=''.join(f'{x:x}\t{y:x}\n' for x,y in cases)
(a.out/'corpus.tsv').write_text(corpus)
expected=''.join(f'{x*y:x}\n' for x,y in cases)
(a.out/'python.txt').write_text(expected)
malachite=subprocess.run([str(a.malachite.resolve()),'mul-oracle',str((a.out/'corpus.tsv').resolve())],check=True,text=True,capture_output=True).stdout
assert malachite==expected
(a.out/'malachite.txt').write_text(malachite)
# Separate functions keep native compiler frame sizes bounded.
source='from std.bigint import parse, BigIntError\n'
for i,(x,y) in enumerate(cases):
 source += f'def case_{i}() -> Result[(), AllocError | BigIntError | IoError]:\n    x = parse("{x:x}",16,100000)?\n    y = parse("{y:x}",16,100000)?\n    p = (x * y)?\n    print(p.to_string(16,100000)?)?\n    Ok(())\n'
source+='def main() -> Result[(), AllocError | BigIntError | IoError]:\n'+''.join(f'    case_{i}()?\n' for i in range(len(cases)))+'    Ok(())\n'
(a.out/'oracle.plenty').write_text(source)
subprocess.run([str(a.compiler.resolve()),'--compile',str(a.out/'oracle.plenty'),'-o',str(a.out/'oracle')],check=True)
actual=subprocess.run([str((a.out/'oracle').resolve())],check=True,text=True,capture_output=True).stdout
(a.out/'plenty.txt').write_text(actual)
assert actual==expected
summary={'cases':len(cases),'seed':987140,'corpus_sha256':hashlib.sha256(corpus.encode()).hexdigest(),'product_sha256':hashlib.sha256(expected.encode()).hexdigest(),'matched':['Python exact integers','Malachite0.14','Plenty']}
(a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(summary)
