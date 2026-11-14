#define _GNU_SOURCE
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
typedef double (*kernel)(const double *,int64_t,int64_t);
static kernel load(const char *path) {
    int fd=open(path,O_RDONLY); assert(fd>=0);
    struct stat s; assert(fstat(fd,&s)==0);
    void *p=mmap(NULL,s.st_size,PROT_READ|PROT_EXEC,MAP_PRIVATE,fd,0);
    assert(p!=MAP_FAILED); close(fd); return (kernel)p;
}
static uint64_t bits(double x) { uint64_t v; memcpy(&v,&x,8); return v; }
int main(int argc,char **argv) {
    assert(argc==3); kernel a=load(argv[1]),b=load(argv[2]);
    const uint64_t cases[]={0,0x8000000000000000ULL,0x3ff0000000000000ULL,0xbff0000000000000ULL,1,0x8000000000000001ULL,0x7ff0000000000000ULL,0xfff0000000000000ULL,0x7ff8000000000001ULL,0xfff8000000001234ULL,0x7ff0000000000001ULL};
    double input[65]; uint64_t state=123456789; size_t count=0;
    for(size_t mode=0;mode<130;mode++) {
        for(size_t j=0;j<65;j++) {
            state^=state<<13; state^=state>>7; state^=state<<17;
            uint64_t v=mode<11?cases[mode]:state; memcpy(input+j,&v,8);
        }
        for(int64_t n=-3;n<=65;n++) for(int64_t row=0;row<4;row++) {
            int64_t i=(int64_t[]){0,1,19,999999}[row];
            uint64_t x=bits(a(input,n,i)),y=bits(b(input,n,i));
            if(x!=y) { fprintf(stderr,"mismatch mode%zu n%ld i%ld: %016lx != %016lx\n",mode,n,i,x,y);return 1; }
            count++;
        }
    }
    printf("%zu bitwise baseline/candidate comparisons passed\n",count);
}
