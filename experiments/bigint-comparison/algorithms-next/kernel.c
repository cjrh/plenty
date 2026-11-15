#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
static uint64_t add(uint64_t x,uint64_t y) { uint64_t z;if(__builtin_add_overflow(x,y,&z))abort();return z; }
static uint64_t sub(uint64_t x,uint64_t y) { uint64_t z;if(__builtin_sub_overflow(x,y,&z))abort();return z; }
static uint64_t mont(uint64_t a,uint64_t b,uint64_t p,uint64_t inverse) {
 __uint128_t t=(__uint128_t)a*b;
 uint64_t lo=(uint64_t)t,hi=(uint64_t)(t>>64),m=lo*inverse;
 __uint128_t mp=(__uint128_t)m*p;
 uint64_t ml=(uint64_t)mp,mh=(uint64_t)(mp>>64),sum=lo+ml;
 uint64_t result=add(add(hi,mh),sum<lo);
 return result>=p?sub(result,p):result;
}
static void forward(uint64_t *data,size_t n,const uint64_t *roots,size_t nr,uint64_t p,uint64_t inverse) {
 for(size_t width=n,step=1;width>=2;width/=2,step*=2) {
  size_t half=width/2;
  for(size_t start=0;start<n;start+=width) {
   if(start+width>n)abort();
   uint64_t *low=data+start,*high=low+half;
   for(size_t j=0,k=0;j<half;j++,k+=step) {
    if(k>=nr)abort();
    uint64_t u=low[j],v=high[j],sum=add(u,v),diff=sub(add(u,p),v);
    low[j]=sum>=p?sub(sum,p):sum;
    high[j]=mont(diff>=p?sub(diff,p):diff,roots[k],p,inverse);
   }
  }
 }
}
int main(int argc,char **argv) {
 if(argc!=2)return 1;
 size_t count=strtoull(argv[1],0,10);
 uint64_t *data=calloc(1024,sizeof(uint64_t)),*roots=calloc(512,sizeof(uint64_t));
 if(!data||!roots)return 1;
 for(size_t i=0;i<1024;i++)data[i]=i;
 for(size_t i=0;i<512;i++)roots[i]=2269391999729648ULL;
 for(size_t i=0;i<count;i++)forward(data,1024,roots,512,1152779667606863873ULL,1152779667606863871ULL);
 printf("%llu\n",(unsigned long long)data[0]);
 free(data);free(roots);
}
