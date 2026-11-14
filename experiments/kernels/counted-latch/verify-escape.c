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
typedef uint8_t (*kernel)(double,double);
static kernel load(const char *path) {
    int fd=open(path,O_RDONLY); assert(fd>=0);
    struct stat s; assert(fstat(fd,&s)==0);
    void *p=mmap(NULL,s.st_size,PROT_READ|PROT_EXEC,MAP_PRIVATE,fd,0);
    assert(p!=MAP_FAILED); close(fd); return (kernel)p;
}
static uint8_t reference(double cr,double ci) {
    double zr=0,zi=0,tr=0,ti=0;
    for(int i=0;i<50;i++) {
        if(!(tr+ti<=4.0)) break;
        zi=2.0*zr*zi+ci;
        zr=tr-ti+cr;
        tr=zr*zr;ti=zi*zi;
    }
    return tr+ti<=4.0;
}
int main(int argc,char **argv) {
    assert(argc==2);kernel functions[12];
    for(size_t i=0;i<12;i++) {
        char path[4096]; int n=snprintf(path,sizeof(path),"%s/0-%zu.bin",argv[1],i);
        assert(n>0&&(size_t)n<sizeof(path)); functions[i]=load(path);
    }
    const uint64_t special[]={0,0x8000000000000000ULL,0x3ff0000000000000ULL,0xbff0000000000000ULL,1,0x8000000000000001ULL,0x7ff0000000000000ULL,0xfff0000000000000ULL,0x7ff8000000000001ULL,0xfff8000000001234ULL,0x7ff0000000000001ULL};
    uint64_t state=123456789;size_t count=0;
    for(size_t i=0;i<40000;i++) {
        double cr,ci;
        if(i<121) { memcpy(&cr,&special[i/11],8);memcpy(&ci,&special[i%11],8); }
        else if(i<20000) {
            state^=state<<13;state^=state>>7;state^=state<<17;memcpy(&cr,&state,8);
            state^=state<<13;state^=state>>7;state^=state<<17;memcpy(&ci,&state,8);
        } else { cr=(double)((int)(i%201)-100)/50;ci=(double)((int)(i/201%201)-100)/50; }
        uint8_t expected=reference(cr,ci);
        for(size_t j=0;j<12;j++) {
            uint8_t got=functions[j](cr,ci);
            if(got!=expected) { fprintf(stderr,"variant%zu case%zu mismatch\n",j,i);return 1; }
            count++;
        }
    }
    printf("%zu C/CLIF escape comparisons passed\n",count);
}
