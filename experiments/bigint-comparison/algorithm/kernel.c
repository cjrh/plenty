#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
static uint64_t add(uint64_t x,uint64_t y) { uint64_t z;if(__builtin_add_overflow(x,y,&z))abort();return z; }
static uint64_t sub(uint64_t x,uint64_t y) { uint64_t z;if(__builtin_sub_overflow(x,y,&z))abort();return z; }
static uint64_t twiddle(uint64_t v,uint64_t root,uint64_t factor,uint64_t p) {
 uint64_t q=v*factor>>32,r=v*root-q*p;
 return r>=p?r-p:r;
}
static void forward(uint64_t *data,size_t n,const uint64_t *roots,size_t nr,uint64_t p) {
 for(size_t width=n,step=1;width>=2;width/=2,step*=2) {
  size_t half=width/2;
  for(size_t start=0;start<n;start+=width) {
   if(start+width>n)abort();
   uint64_t *low=data+start,*high=low+half;
   for(size_t j=0,k=0;j<half;j++,k+=step) {
    if(k>=nr/2)abort();
    uint64_t u=low[j],v=high[j],sum=add(u,v),diff=sub(add(u,p),v);
    low[j]=sum>=p?sub(sum,p):sum;
    high[j]=twiddle(diff>=p?sub(diff,p):diff,roots[k],roots[nr/2+k],p);
   }
  }
 }
}
int main(int argc,char **argv) {
 if(argc!=2)return 1;
 size_t count=strtoull(argv[1],0,10);
 uint64_t *data=calloc(8192,sizeof(uint64_t)),*roots=calloc(8192,sizeof(uint64_t));
 if(!data||!roots)return 1;
 for(size_t i=0;i<8192;i++)data[i]=i%998244353;
 for(size_t i=0;i<8192;i++)roots[i]=i<4096?1:4;
 for(size_t i=0;i<count;i++)forward(data,8192,roots,8192,998244353);
 printf("%llu\n",(unsigned long long)data[0]);
 free(data);free(roots);
}
