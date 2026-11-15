#!/usr/bin/env python3
"""Certify independently generated primes, exact Montgomery/Garner arithmetic,
and the published Toom-3 interpolation identities used by std.bigint.
"""
import json
import math
from pathlib import Path
import random
import re

R = 1 << 64
MASK = R - 1
primes = []
# Proth's theorem supplies exact certificates: k odd, k<2^40 and
# a^((p-1)/2)=-1 mod p prove that p=k*2^40+1 is prime.
for k in range((1 << 20) - 1, 1, -2):
    p = k * (1 << 40) + 1
    witness = next((a for a in range(2, 32) if pow(a, (p-1)//2, p) == p-1), None)
    if witness is not None:
        primes.append(dict(p=p, k=k, witness=witness,
                           inverse=(-pow(p,-1,R)) % R,
                           r=R % p, r2=R**2 % p, r3=R**3 % p))
        if len(primes) == 5:
            break
source = (Path(__file__).resolve().parents[3] / 'stdlib/std/bigint.plenty').read_text()
calls = [line for line in source.splitlines() if line.strip().startswith('transform_wide_product(a, b,')]
expected = [[d[key] for key in ('p','witness','inverse','r','r2','r3')] for d in primes]
assert [[int(n) for n in re.findall(r'(\d+)u64', line)] for line in calls] == expected
# Verify the literal mixed-radix inverses too, rather than proving a parallel
# reconstruction which could silently diverge from production constants.
garner_lines = '\n'.join(line for line in source.splitlines()
                         if line.strip().startswith(('c1 =', 'c2 =', 'c3 =', 'c4 ='))
                         and 'transform_garner_step(' in line)
garner_constants = [tuple(map(int, values)) for values in re.findall(
    r', (\d+)u64, (\d+)u64, (\d+)u64, (\d+)u64\)', garner_lines)]
assert garner_constants == [
    (pow(primes[j]['p'], -1, primes[i]['p']), primes[i]['p'],
     primes[i]['inverse'], primes[i]['r2'])
    for i in range(1, 5) for j in range(i)
]
rng = random.Random(987152)
for d in primes:
    p, inv = d['p'], d['inverse']
    assert d['k'] % 2 and d['k'] < (1 << 40)
    assert pow(d['witness'], (p-1)//2, p) == p-1
    assert inv == p-2
    for n in (2, 1024, 1 << 18, 1 << 21, 1 << 40):
        root = pow(d['witness'], (p-1)//n, p)
        assert pow(root,n,p) == 1 and pow(root,n//2,p) == p-1
    def reduce(low, high):
        m = low * inv & MASK
        z = low + m*p
        assert z & MASK == 0
        result = high + (z >> 64)
        assert result < 2*p < (1 << 61)
        return result-p if result >= p else result
    def mul(a,b):
        t = a*b
        return reduce(t & MASK, t >> 64)
    rinv = pow(R,-1,p)
    for _ in range(20000):
        lo,hi = rng.randrange(R), rng.randrange(p)
        assert reduce(lo,hi) == (lo+hi*R)*rinv % p
        a,b = rng.randrange(p),rng.randrange(p)
        assert mul(a,b) == a*b*rinv % p
        high = rng.randrange(R)
        assert mul(reduce(lo,high % p),d['r3']) == (lo+high*R)*R % p
bound = (1 << 20) * ((1 << 128)-1)**2
modulus = math.prod(d['p'] for d in primes)
assert bound < modulus < (1 << 300)
for _ in range(20000):
    x = rng.randrange(bound)
    cs=[]
    for i,d in enumerate(primes):
        p=d['p'];c=x % p
        for j in range(i):
            c=(c-cs[j])*pow(primes[j]['p'],-1,p) % p
        cs.append(c)
    value=cs[-1]
    for j in range(3,-1,-1):
        value=value*primes[j]['p']+cs[j]
    assert value==x
    carry=rng.randrange(1 << 148)
    limbs=[(value >> (64*i)) & MASK for i in range(5)]
    incoming=[(carry >> (64*i)) & MASK for i in range(3)] + [0,0]
    overflow=0;result=[]
    for a,b in zip(limbs,incoming):
        total=a+b+overflow
        result.append(total & MASK);overflow=total >> 64
    assert overflow==0
    assert sum(word << (64*i) for i,word in enumerate(result)) == x+carry
    assert sum(word << (64*i) for i,word in enumerate(result[2:])) == (x+carry) >> 128
r=rng
# Verify the independent interpolation identities for nonnegative chunks,
# including negative values at -1 and exact divisions by two and three.
for _ in range(20000):
    a0,a1,a2,b0,b1,b2 = [rng.getrandbits(128) for _ in range(6)]
    v0,v4 = a0*b0,a2*b2
    v1 = (a0+a1+a2)*(b0+b1+b2)
    vm = (a0-a1+a2)*(b0-b1+b2)
    v2 = (a0+2*a1+4*a2)*(b0+2*b1+4*b2)
    assert (v1-vm) % 2 == 0
    odd = (v1-vm)//2
    quadratic = v1-v0-v4-odd
    assert (v2-v0-16*v4) % 2 == 0
    numerator = (v2-v0-16*v4)//2-odd-2*quadratic
    assert numerator % 3 == 0
    cubic = numerator//3
    linear = odd-cubic
    assert (linear,quadratic,cubic) == (a0*b1+a1*b0,a0*b2+a1*b1+a2*b0,a1*b2+a2*b1)
print(json.dumps({'seed':987152,'primes':primes,'coefficient_bound':bound,
                  'modulus_product':modulus,'montgomery_cases_per_prime':20000,
                  'garner_carry_cases':20000,'toom_interpolation_cases':20000},indent=2))
