// n-body with a `Vec<Body>` whose length is known only at run time: the Rust
// counterpart of nbody_clist.plenty and nbody.plenty. Same statement order as
// the Plenty sources; every index is bounds-checked.
use std::f64::consts::PI;
use std::hint::black_box;

const SOLAR_MASS: f64 = 4.0 * PI * PI;
const DAYS: f64 = 365.24;

#[derive(Clone, Copy)]
struct Body { x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64 }

fn planet(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64) -> Body {
    Body { x, y, z, vx: vx * DAYS, vy: vy * DAYS, vz: vz * DAYS, mass: mass * SOLAR_MASS }
}

fn starting_state() -> Vec<Body> {
    let mut b = Vec::with_capacity(5);
    b.push(Body { x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: SOLAR_MASS });
    b.push(planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
        1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04));
    b.push(planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
        -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04));
    b.push(planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
        2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05));
    b.push(planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
        2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05));
    b
}

fn offset_momentum(b: &mut [Body]) {
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for i in 0..b.len() {
        px += b[i].vx * b[i].mass;
        py += b[i].vy * b[i].mass;
        pz += b[i].vz * b[i].mass;
    }
    b[0].vx = -px / SOLAR_MASS;
    b[0].vy = -py / SOLAR_MASS;
    b[0].vz = -pz / SOLAR_MASS;
}

fn energy(b: &[Body]) -> f64 {
    let mut e = 0.0;
    for i in 0..b.len() {
        e += 0.5 * b[i].mass * (b[i].vx * b[i].vx + b[i].vy * b[i].vy + b[i].vz * b[i].vz);
        for j in i + 1..b.len() {
            let (dx, dy, dz) = (b[i].x - b[j].x, b[i].y - b[j].y, b[i].z - b[j].z);
            e -= b[i].mass * b[j].mass / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}

#[inline(never)]
fn advance(b: &mut Vec<Body>, dt: f64, steps: usize) {
    let n = b.len();
    let pairs = n * (n - 1) / 2;
    let mut dx = vec![0.0; pairs];
    let mut dy = vec![0.0; pairs];
    let mut dz = vec![0.0; pairs];
    let mut mag = vec![0.0; pairs];
    for _ in 0..steps {
        let mut k = 0;
        for i in 0..n {
            for j in i + 1..n {
                dx[k] = b[i].x - b[j].x;
                dy[k] = b[i].y - b[j].y;
                dz[k] = b[i].z - b[j].z;
                k += 1;
            }
        }
        for p in 0..pairs {
            let d2 = dx[p] * dx[p] + dy[p] * dy[p] + dz[p] * dz[p];
            mag[p] = dt / (d2 * d2.sqrt());
        }
        k = 0;
        for i in 0..n {
            for j in i + 1..n {
                let mi = b[i].mass * mag[k];
                let mj = b[j].mass * mag[k];
                b[i].vx -= dx[k] * mj;
                b[i].vy -= dy[k] * mj;
                b[i].vz -= dz[k] * mj;
                b[j].vx += dx[k] * mi;
                b[j].vy += dy[k] * mi;
                b[j].vz += dz[k] * mi;
                k += 1;
            }
        }
        for i in 0..n {
            b[i].x += dt * b[i].vx;
            b[i].y += dt * b[i].vy;
            b[i].z += dt * b[i].vz;
        }
    }
}

fn main() {
    let steps = std::env::args().nth(1).map_or(1000, |s| s.parse().unwrap());
    let mut b = black_box(starting_state());
    offset_momentum(&mut b);
    println!("{:.9}", energy(&b));
    advance(&mut b, 0.01, steps);
    println!("{:.9}", energy(&b));
}
