# Find a route through a graph

`std.graph` stores nodes and the edges between them. `UnGraph` edges join two
nodes both ways; `DiGraph` edges run from a source to a target. Each node and
edge owns a value, and adding one returns a handle that names it.

```plenty
from std.graph import UnGraph, GraphError

def minutes(walk: &i64) -> f64:
    f64(*walk)

def main() -> Result[(), AllocError | IoError | GraphError]:
    mut town = UnGraph[str, i64]()?
    home = town.add_node("home")?
    shop = town.add_node("shop")?
    park = town.add_node("park")?
    school = town.add_node("school")?
    town.add_edge(home, shop, 10)?
    town.add_edge(home, park, 4)?
    town.add_edge(park, shop, 3)?
    town.add_edge(shop, school, 6)?
    paths = town.dijkstra(home, &minutes)?
    for stop in paths.path(school)?:
        print(*town.node(stop))?
    print(paths.distance(school))?
    Ok(())
```
```output
home
park
shop
school
Option[f64].Some(13.0)
```

The graph does not know what edge values mean, so `dijkstra` takes a function
that turns one into a non-negative weight. The result answers distance and
path queries for every node reachable from the start.

Handles stay valid after removals: removing `park` leaves `shop` and `school`
naming the same places, and the path now takes the direct road.

```plenty
from std.graph import UnGraph, GraphError

def minutes(walk: &i64) -> f64:
    f64(*walk)

def main() -> Result[(), AllocError | IoError | GraphError]:
    mut town = UnGraph[str, i64]()?
    home = town.add_node("home")?
    shop = town.add_node("shop")?
    park = town.add_node("park")?
    town.add_edge(home, shop, 10)?
    town.add_edge(home, park, 4)?
    town.add_edge(park, shop, 3)?
    print(town.remove_node(park))?
    print(town.contains_node(park), town.edge_count())?
    print(town.get_node_ref(park), town.get_node_ref(shop))?
    print(town.dijkstra(home, &minutes)?.distance(shop))?
    Ok(())
```
```output
Option[str].Some("park")
False 1
Option[&str].Nothing Option[&str].Some("shop")
Option[f64].Some(10.0)
```

Using a removed handle with `node` or `add_edge` traps. When a handle may be
stale, `get_node_ref` and `get_edge_ref` return `Nothing` instead of trapping,
and `get_node_mut` and `get_edge_mut` do the same for changes. See
[the module reference](../../design/stdlib/graph.md) for traversal,
topological order, components, spanning trees and costs.
