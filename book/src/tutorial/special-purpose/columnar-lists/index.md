# Columnar lists

`clist[T]` lets you declare ordinary classes while storing each field in its own
contiguous column. This layout is called **structure of arrays (SoA)**, in
contrast to **array of structures (AoS)**, which groups fields by record. See
[Wikipedia's AoS and SoA explanation](https://en.wikipedia.org/wiki/AoS_and_SoA).
The storage choice belongs to this collection; it does not change how `T` is
stored everywhere else.

Choose it when most passes visit many records but use only a few fields.
For example, a simulation updates positions from velocities without reading
mass or temperature; a wide dataset can have dozens of fields while each
calculation needs only one or two. Contiguous columns bring more useful values
into the CPU cache with each memory access. Ordinary scalar scans benefit from
this layout too; SIMD is an additional optimization for supported calculations.

## Choose an idiom

| Task | Using `list` | Using `clist` | Loop instructions, `list` / `clist` |
| --- | --- | --- | --- |
| [Aggregate one field](#aggregate-one-field) | Borrow rows and add the field | Select a column and `fold` | 6 / 5 |
| [Filter and aggregate two fields](#filter-and-aggregate-two-fields) | Test and sum fields of borrowed rows | Zip the selected columns and scan their scalar values | 9 / 8 |
| [Update positions](#update-positions-from-velocities) | Mutably borrow each row | Loop over indices and update selected fields | 15 / 14 |

The counts summarize optimized standalone loops for the eight-field record
below, on x86-64. They measure generated work, **not elapsed-time speedup** or
cache misses. See [measurement conditions](#measurement-conditions).

The examples share `particles.plenty`, shown below, and define one module per
scenario. [Run them together](#run-the-examples) after saving the four files.
The eight-field record deliberately contains fields that each pass leaves
untouched. The two constructors supply identical small inputs for the examples; use
[reserved construction](01-scanning.md) when preparing a larger collection.

```plenty-file particles.plenty
pub class Particle:
    pub x: f64
    pub y: f64
    pub vx: f64
    pub vy: f64
    pub mass: f64
    pub temperature: f64
    pub health: i32
    pub team: i32

pub def example_list() -> Result[list[Particle], AllocError]:
    [Particle(1.0, 2.0, 0.5, 1.0, 3.0, 10.0, 100i32, 0i32),
     Particle(4.0, 5.0, 1.0, 0.5, 7.0, 30.0, 50i32, 1i32)]

pub def example_clist() -> Result[clist[Particle], AppendError[Particle]]:
    mut particles = clist[Particle]()
    particles.append(Particle(1.0, 2.0, 0.5, 1.0, 3.0, 10.0, 100i32, 0i32))?
    particles.append(Particle(4.0, 5.0, 1.0, 0.5, 7.0, 30.0, 50i32, 1i32))?
    Ok(particles)
```

## Aggregate one field

For a list, borrow each object and read its mass. For a clist, select the mass
column and fold its scalar values. Neither version copies whole objects or
allocates during the calculation. The clist scan reads no other field.

```plenty-file aggregate.plenty
from particles import Particle

pub def list_mass(particles: &list[Particle]) -> f64:
    mut total = 0.0
    for particle in particles:
        total += particle.mass
    total

def add_mass(total: f64, mass: f64) -> f64:
    total + mass

pub def clist_mass(particles: &clist[Particle]) -> f64:
    particles.column(Particle.mass).fold(0.0, add_mass)
```

## Filter and aggregate two fields

To sum the mass of particles at or above a temperature threshold, the list
version inspects both fields of each borrowed row. The clist version selects
the temperature and mass columns, then zips their scalar values in row order.
The compiler prepares both column addresses before the loop and scans them
with one shared cursor.

```plenty-file conditional.plenty
from particles import Particle

pub def list_hot_mass(particles: &list[Particle], threshold: f64) -> f64:
    mut total = 0.0
    for particle in particles:
        if particle.temperature >= threshold:
            total += particle.mass
    total

pub def clist_hot_mass(particles: &clist[Particle], threshold: f64) -> f64:
    mut total = 0.0
    temperatures = particles.column(Particle.temperature)
    masses = particles.column(Particle.mass)
    for temperature, mass in temperatures.zip(masses):
        if temperature >= threshold:
            total += mass
    total
```

These floating-point calculations preserve row order and ordinary arithmetic.
They use scalar execution, independently of the specialized
[integer filter/count SIMD](02-counting.md). Finish a column pipeline before
mutating its owner: it borrows the entire collection, even if it selects only
one field. Columns from the same collection have equal lengths and matching
row order, so `zip` pairs fields from the same record. Use this form for a
read-only multi-field pass; use indexed field writes when updating records.
A conditional `fold` over the same zipped columns is also supported; its
callback takes the accumulator followed by the two scalar values.

## Update positions from velocities

Borrow list rows mutably to change their fields. For a clist, use an indexed
loop: it reads and writes packed scalar fields directly. There is no need to
copy a row, edit the copy, and write it back. In particular, `copy_row` produces
an independent object whose changes do not update the collection.

```plenty-file advance.plenty
from particles import Particle

pub def list_advance(particles: &mut list[Particle], dt: f64) -> ():
    for particle in particles:
        particle.x += dt * particle.vx
        particle.y += dt * particle.vy

pub def clist_advance(particles: &mut clist[Particle], dt: f64) -> ():
    for i in range(len(particles)):
        particles[i].x += dt * particles[i].vx
        particles[i].y += dt * particles[i].vy
```

The compiler caches the row count and accessed column addresses before this
loop. Keeping structural changes outside the pass permits this optimization;
loops that can resize or replace the owner use per-access metadata loads.
Field writes themselves do not resize it. This pass touches four columns,
regardless of how many other fields the class has.

This is useful for a simulation with a fixed record schema, and can provide
storage for an entity component system (ECS). A clist alone does not provide
stable entity handles, queries over changing component sets, or a scheduler.
[Ordered removal](03-removing.md) shifts later row indices; an index is not a
persistent entity identity.

## Run the examples

Save this program beside `particles.plenty`, `aggregate.plenty`,
`conditional.plenty`, and `advance.plenty`. Both collections start with the same
two records; each pair of functions produces the same result.

```plenty
from particles import Particle, example_list, example_clist
from aggregate import list_mass, clist_mass
from conditional import list_hot_mass, clist_hot_mass
from advance import list_advance, clist_advance

def main() -> Result[(), AllocError | AppendError[Particle] | IoError]:
    mut rows = example_list()?
    mut columns = example_clist()?
    print(list_mass(&rows), clist_mass(&columns))?
    print(list_hot_mass(&rows, 20.0), clist_hot_mass(&columns, 20.0))?
    list_advance(&mut rows, 2.0)
    clist_advance(&mut columns, 2.0)
    print(rows[0].x, rows[0].y, columns[0].x, columns[0].y)?
    Ok(())
```
```output
10.0 10.0
7.0 7.0
2.0 4.0 2.0 4.0
```

## Measurement conditions

The table counts instructions on one loop pass using Plenty 0.8.19, compiler
source commit `c63d0884`, on x86-64. Conditional aggregation counts the path
that includes the mass. None of the recommended clist loops reconstructs rows
or calls a generic field-access helper; column scans and indexed update loops
prepare their column addresses before scanning. The benchmark runner extracts the exact function
bodies shown here and also checks a 64-field schema with 56 additional unused
fields. Those fields are not read by these passes.

Instruction counts cannot establish a runtime speedup: cache locality, row
width, working-set size, setup costs and code placement also matter. No elapsed
time comparison is claimed here because concurrent builds prevented an
uncontended measurement window. In particular, these counts do not quantify
the expected memory-traffic advantage on wide records. Measure representative
inputs before choosing a layout for a performance-critical application.

The [idiom measurements](../../../proposals/columnar-list/idiom-measurements.md) describe the
scalar C comparison, independent checksum checks, the delivered shared-cursor
and immutable-capture optimizations (issues #888 and #891), and commands for
repeating timing runs on an idle machine.

## Construction, counting, and removal

The current implementation accepts only primitive numeric and boolean fields.
Use an ordinary list for strings, nested objects, borrowed row references, or
workloads better served by whole-object access. Column iteration yields scalar
values; general row iteration and mutable column views are not supported.
For analytical expression plans, grouping and Parquet datasets, consider
[dataframes](../dataframes.md).

- [Build and scan columnar records](01-scanning.md): reserve capacity, append
  records, scan a column, and update or copy a row.
- [Count matching integer values](02-counting.md): express a filter/count
  calculation that the compiler can accelerate with SIMD.
- [Remove rows and reuse storage](03-removing.md): take individual records or
  clear a batch while retaining capacity.

See the [columnar-list reference](../../../design/facilities/01-builtins/clists.md)
for supported types, ownership, allocation failures, and operation costs.
