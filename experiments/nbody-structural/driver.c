/* Shared executable driver: identical initial state for each C/CLIF kernel.
 * Initialization matches Plenty's momentum accumulation, not Rust's slightly
 * different (mathematically equivalent) expression order.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { double x, y, z, vx, vy, vz, mass; } Body;
extern void KERNEL(Body *, int64_t);
static double energy(Body *b) {
    double e = 0;
    for (int i=0; i<5; ++i) {
        e += 0.5*b[i].mass*(b[i].vx*b[i].vx+b[i].vy*b[i].vy+b[i].vz*b[i].vz);
        for (int j=i+1; j<5; ++j) {
            double dx=b[i].x-b[j].x,dy=b[i].y-b[j].y,dz=b[i].z-b[j].z;
            e -= b[i].mass*b[j].mass/sqrt(dx*dx+dy*dy+dz*dz);
        }
    }
    return e;
}
int main(int argc, char **argv) {
    int64_t steps = argc > 1 ? strtoll(argv[1], 0, 10) : 1000;
    const double solar=4.0*3.141592653589793*3.141592653589793;
    Body b[5] = {
        {0,0,0,0,0,0,1},
        {4.84143144246472090e+00,-1.16032004402742839e+00,-1.03622044471123109e-01,1.66007664274403694e-03,7.69901118419740425e-03,-6.90460016972063023e-05,9.54791938424326609e-04},
        {8.34336671824457987e+00,4.12479856412430479e+00,-4.03523417114321381e-01,-2.76742510726862411e-03,4.99852801234917238e-03,2.30417297573763929e-05,2.85885980666130812e-04},
        {1.28943695621391310e+01,-1.51111514016986312e+01,-2.23307578892655734e-01,2.96460137564761618e-03,2.37847173959480950e-03,-2.96589568540237556e-05,4.36624404335156298e-05},
        {1.53796971148509165e+01,-2.59193146099879641e+01,1.79258772950371181e-01,2.68067772490389322e-03,1.62824170038242295e-03,-9.51592254519715870e-05,5.15138902046611451e-05}
    };
    double px=0,py=0,pz=0;
    for(int i=0;i<5;++i) {
        b[i].vx*=365.24; b[i].vy*=365.24; b[i].vz*=365.24; b[i].mass*=solar;
        px+=b[i].vx*b[i].mass; py+=b[i].vy*b[i].mass; pz+=b[i].vz*b[i].mass;
    }
    b[0].vx=-px/solar; b[0].vy=-py/solar; b[0].vz=-pz/solar;
    printf("%.9f\n",energy(b));
    KERNEL(b,steps);
    printf("%.9f\n",energy(b));
    if (argc > 2 && strcmp(argv[2], "state") == 0) {
        for (int i=0;i<5;++i)
            printf("%.17e %.17e %.17e %.17e %.17e %.17e %.17e\n",
                   b[i].x,b[i].y,b[i].z,b[i].vx,b[i].vy,b[i].vz,b[i].mass);
    }
    return 0;
}
