#!/usr/bin/env python3
"""Check mathematical parameter contracts independently of Plenty execution."""
import json
import random
p,q=998244353,1004535809
for prime in (p,q):
    assert all(prime%d for d in range(2,int(prime**.5)+1))
    root=pow(3,(prime-1)//(1<<21),prime)
    assert pow(root,1<<21,prime)==1 and pow(root,1<<20,prime)!=1
assert p*q > (1<<20)*65535**2
assert p*669690699%q==1
r=random.Random(987142)
for prime in (p,q):
    inverse=(-pow(prime,-1,1<<32))% (1<<32)
    for i in range(10000):
        value=r.randrange(prime);root=r.randrange(1,prime)
        factor=(root<<32)//prime
        estimate=value*factor>>32
        residual=value*root-estimate*prime
        assert 0<=residual<2*prime
        assert (residual-prime if residual>=prime else residual)==value*root%prime
        assert ((prime-root)<<32)//prime==(1<<32)-1-factor
        t=value*root;m=(t*inverse)&((1<<32)-1)
        assert t+m*prime < 1<<63
        reduced=(t+m*prime)>>32
        assert (reduced-prime if reduced>=prime else reduced)==t*pow(1<<32,-1,prime)%prime
print(json.dumps({'primes':[p,q],'primitive_root':3,'common_order':1<<21,'maximum_coefficient':(1<<20)*65535**2,'crt_product':p*q,'random_reduction_pairs':20000,'seed':987142},indent=2))
