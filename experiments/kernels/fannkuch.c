/* fannkuch-redux as examples/fannkuch_redux.plenty computes it: 8 workers,
   128 blocks, the same checksum and output. See README.md. */
#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
typedef long long i64;
static i64 n, total, size; enum {W=8, B=128};
static i64 fact(i64 k){i64 r=1;for(i64 i=2;i<=k;i++)r*=i;return r;}
static i64 res[B][2];
static void rot(i64*p,i64 last){i64 h=p[0];for(i64 i=0;i<last;i++)p[i]=p[i+1];p[last]=h;}
static void block(i64 b){i64 start=b*size, stop=start+size<total?start+size:total;
 i64 perm[16],s[16],c[16]; for(i64 i=0;i<n;i++){perm[i]=i;c[i]=0;}
 i64 idx=start; for(i64 i=n-1;i>=1;i--){i64 pl=fact(i),d=idx/pl;idx%=pl;c[i]=d;for(i64 k=0;k<d;k++)rot(perm,i);}
 i64 cs=0,mx=0,pos=start; if(start>=stop)return;
 for(;;){i64 f=0,first=perm[0]; if(first){for(i64 i=0;i<n;i++)s[i]=perm[i];
   while(first){i64 lo=1,hi=first-1;while(lo<hi){i64 t=s[lo];s[lo]=s[hi];s[hi]=t;lo++;hi--;}i64 nx=s[first];s[first]=first;first=nx;f++;}}
  if(f>mx)mx=f; cs+= pos%2==0?f:-f; pos++; if(pos==stop)break;
  i64 i=1; for(;;){rot(perm,i);c[i]++;if(c[i]<=i)break;c[i]=0;i++;}}
 res[b][0]=cs;res[b][1]=mx;}
static i64 next=0; static pthread_mutex_t m=PTHREAD_MUTEX_INITIALIZER;
static void*work(void*_){for(;;){pthread_mutex_lock(&m);i64 b=next++;pthread_mutex_unlock(&m);if(b>=B)return 0;block(b);}}
int main(int c,char**v){n=c>1?atoi(v[1]):7;total=fact(n);size=(total+B-1)/B;pthread_t t[W];
 for(int i=0;i<W;i++)pthread_create(&t[i],0,work,0);for(int i=0;i<W;i++)pthread_join(t[i],0);
 i64 cs=0,mx=0;for(int b=0;b<B;b++){cs+=res[b][0];if(res[b][1]>mx)mx=res[b][1];}
 printf("%lld\nPfannkuchen(%lld) = %lld\n",cs,n,mx);}
