# std.graph

`from std.graph import DiGraph, UnGraph` provides owned directed and
undirected graphs that store a value of type `N` on every node and `E` on
every edge, with traversal, shortest-path, ordering, component and spanning
tree algorithms. It is ordinary embedded Plenty code over lists; there is no
native dependency.

## Graph kinds

| Type | Edges | Algorithms only on this kind |
|---|---|---|
| `DiGraph[N, E]` | Run from `source` to `target` | `toposort`, `strongly_connected_components`, `predecessors`, `incoming` |
| `UnGraph[N, E]` | Join two endpoints symmetrically | `connected_components`, `minimum_spanning_tree`, `endpoints`, `opposite` |

The kind is part of the type, so calling a directed-only algorithm on an
undirected graph is a compile-time error rather than a runtime surprise.
Both kinds permit parallel edges and self-loops. `N` and `E` follow ordinary
collection storage rules: affine owners are allowed, references and generators
are not, and unit `()` is not yet a supported type argument; use a small
placeholder such as `bool` for a graph without data.

## Handles

`NodeId` and `EdgeId` are public aliases of `i64`. The n-th node or edge added
to a graph has handle n, counting from zero. Handles are plain integers so
that they copy; nothing stops passing an `EdgeId` where a `NodeId` is
expected, or a handle from one graph to another.

**Removal never moves or reuses a handle.** `remove_node` and `remove_edge`
leave a vacant slot, so every other handle keeps naming the same node or edge
and a removed handle stays invalid for the graph's lifetime. The alternative,
swap-removal as in petgraph's `Graph`, silently renames the last node to the
removed handle, so a handle saved earlier refers to a different node with no
way to detect it. Reusing vacant slots would need a generation counter in each
handle to catch stale ones. The cost of this rule is that memory stays
proportional to the number of nodes and edges ever added, not the number
alive; a graph with heavy churn should be rebuilt.

`node_count()` and `edge_count()` count live entries. `node_bound()` and
`edge_bound()` are one past the largest handle issued, so
`for node in range(graph.node_bound())` with `contains_node(node)` visits live
nodes without allocating. `nodes()` and `edges()` return live handles in
increasing order.

Operations that take a handle trap when it is negative, out of range, or
removed, as indexing does. The trap is an index-out-of-bounds report inside
`std.graph` with a `note: called from` line naming the caller. `remove_node`,
`remove_edge`, `contains_node`, `contains_edge` and the `get_*` getters never
trap: removal of an invalid handle returns `Nothing`, and so does a checked
getter, so a possibly stale handle needs one lookup rather than a
`contains_node` test followed by `node`.

## Building and reading

| Operation | Result | Contract |
|---|---|---|
| `DiGraph[N, E]()`, `UnGraph[N, E]()` | `Result[Graph, AllocError]` | An empty graph |
| `graph.reserve(nodes, edges)` | `Result[(), AllocError]` | Room for that many more nodes and edges |
| `graph.add_node(data)` | `Result[NodeId, AllocError]` | Consume `data` into a new node |
| `graph.add_edge(a, b, data)` | `Result[EdgeId, AllocError]` | Consume `data` into a new edge; traps for an invalid endpoint |
| `graph.remove_node(node)` | `Option[N]` | Remove every incident edge, then the node, transferring its data |
| `graph.remove_edge(edge)` | `Option[E]` | Remove the edge, transferring its data |
| `graph.node(node)`, `graph.edge(edge)` | `&N`, `&E` | Borrow the data |
| `graph.node_mut(node)`, `graph.edge_mut(edge)` | `&mut N`, `&mut E` | Borrow the data exclusively |
| `graph.get_node_ref(node)`, `graph.get_edge_ref(edge)` | `Option[&N]`, `Option[&E]` | Borrow the data; `Nothing` for an invalid handle |
| `graph.get_node_mut(node)`, `graph.get_edge_mut(edge)` | `Option[&mut N]`, `Option[&mut E]` | Borrow the data exclusively; `Nothing` for an invalid handle |
| `graph.find_edge(a, b)` | `Option[EdgeId]` | The earliest live edge from `a` to `b` (either way for `UnGraph`) |
| `di.source(edge)`, `di.target(edge)` | `NodeId` | Endpoints of a directed edge |
| `un.endpoints(edge)` | `(NodeId, NodeId)` | Endpoints in the order given to `add_edge` |
| `un.opposite(edge, node)` | `NodeId` | The endpoint that is not `node` (`node` itself for a self-loop) |
| `di.successors(node)`, `di.predecessors(node)` | `Result[list[NodeId], AllocError]` | Targets of outgoing edges, sources of incoming edges |
| `un.neighbors(node)` | `Result[list[NodeId], AllocError]` | The other endpoint of every incident edge |

Neighbor lists follow edge insertion order and repeat a node once per parallel
edge. An undirected self-loop appears once in its node's neighbors; a directed
self-loop appears in both `successors` and `predecessors`.

Each node heads linked chains threaded through its edges, as in petgraph's
adjacency list. Adding a node or an edge appends one slot and allocates only
when that list grows, doubling its capacity. Allocation failure leaves the
graph unchanged and drops the consumed data. Lookups, mutation through
references, removal and walks allocate nothing. Removing an edge takes time
proportional to the degrees of its endpoints; removing a node takes time
proportional to the degrees of the node and its neighbors.

`copy(graph)?` duplicates the whole graph, including vacant slots, so handles
remain valid in the copy. Graphs move like other owned collections.

### Allocation-free walks

`di.outgoing(node)`, `di.incoming(node)` and `un.incident(node)` return a
`Walk`: a small cursor that holds no borrow of the graph. Pass it back with
`graph.next_edge(&mut walk)`, which returns `Option[EdgeId]` in insertion
order:

```plenty
from std.graph import UnGraph

def main() -> Result[(), AllocError | IoError]:
    mut roads = UnGraph[str, i64]()?
    home = roads.add_node("home")?
    shop = roads.add_node("shop")?
    park = roads.add_node("park")?
    roads.add_edge(home, shop, 4)?
    roads.add_edge(park, home, 2)?
    mut walk = roads.incident(home)
    while True:
        match roads.next_edge(&mut walk):
            case Some(edge):
                other = roads.opposite(edge, home)
                print(*roads.node(other), *roads.edge(edge))?
            case Nothing:
                break
    Ok(())
```
```output
shop 4
park 2
```

Edges added after the walk began at the end of the chain are seen; removing
the edge the walk would visit next ends the walk early.

## Traversal and components

| Operation | Result | Order |
|---|---|---|
| `graph.bfs(start)` | `Result[list[NodeId], AllocError]` | Breadth-first, neighbors in insertion order |
| `graph.dfs(start)` | `Result[list[NodeId], AllocError]` | Recursive depth-first preorder |
| `un.connected_components()` | `Result[list[list[NodeId]], AllocError]` | Components by smallest handle, each in `bfs` order |
| `di.strongly_connected_components()` | `Result[list[list[NodeId]], AllocError]` | Reverse topological: a component precedes those that reach it |

Traversal returns every node reachable from `start`, which comes first. Directed
traversal follows outgoing edges. `dfs` produces the order of a recursive search
but uses an explicit stack, so a path of any length cannot overflow the call
stack. Strongly connected components use iterative Tarjan; each component lists its
nodes in discovery order. Roots are tried in handle order. Each algorithm takes
O(V + E) time and O(V) extra space, where V is `node_bound()`.

## Topological order

`di.toposort()` returns `Result[list[NodeId], AllocError | GraphError]`. It
uses Kahn's algorithm, starting from source nodes in handle order, so the
result is deterministic. If the graph has a cycle, the error is
`GraphError.Cycle(nodes)`: a list `[a, b, ..., z]` such that the graph has
edges `a -> b`, ..., `z -> a`. A self-loop reports `[node]`. Finding the cycle
takes O(V + E) time after the sort fails.

## Shortest paths

Weights come from a callback, `weight: &F` with `F: Callable[[&E], f64]`, so
edge data can hold anything and the same graph can be searched by several
metrics. Integer weights convert with `f64(value)`; they are exact up to 2^53.

| Operation | Result | Weights |
|---|---|---|
| `graph.dijkstra(start, weight)` | `Result[Paths, AllocError \| GraphError]` | Non-negative; `+inf` acts as no edge |
| `graph.astar(start, goal, weight, estimate)` | `Result[Option[(f64, list[NodeId])], AllocError \| GraphError]` | As Dijkstra |
| `graph.bellman_ford(start, weight)` | `Result[Paths, AllocError \| GraphError]` | Any value except NaN and `-inf` |

`Paths` answers queries for every node:

| Method | Result |
|---|---|
| `paths.distance(node)` | `Option[f64]`: total weight, or `Nothing` when unreachable |
| `paths.edge_to(node)` | `Option[EdgeId]`: the last edge on the path; `Nothing` for the start |
| `paths.path(node)` | `Result[list[NodeId], AllocError]`: start to `node`, or empty when unreachable |

Undirected edges can be used in either direction. Handles added after the
search are reported unreachable.

Dijkstra checks each weight when it first examines that edge and returns
`GraphError.InvalidWeight(edge)` for a negative or NaN weight. It uses a
binary heap from [`std.collections`](collections.md) and takes
O((V + E) log E) time. `astar` stops at `goal` and returns the cost and path,
or `Nothing` when `goal` is unreachable. `estimate: &H` with
`H: Callable[[NodeId], f64]` must never exceed the true remaining cost for
the result to be shortest; a NaN or negative estimate returns
`GraphError.InvalidEstimate(node)`. Nodes may be expanded more than once when
an admissible estimate is inconsistent.

Bellman–Ford allows negative weights and takes O(V × E) time. A negative cycle
reachable from `start` returns `GraphError.NegativeCycle(nodes)` in the same
form as `Cycle`. In an undirected graph, any negative edge reachable from
`start` is such a cycle, reported as its two endpoints.

## Minimum spanning tree

`un.minimum_spanning_tree(weight)` returns `Result[list[EdgeId], AllocError |
GraphError]`: a minimum spanning forest, with one tree per connected
component. It uses Kruskal's algorithm with a union–find, so edges come out in
increasing weight, ties in handle order. Self-loops are never chosen. Negative
weights are allowed; a NaN weight returns `GraphError.InvalidWeight(edge)`.
It takes O(E log E) time and O(V + E) space.

## Errors

`GraphError` has four variants:

| Variant | Raised by |
|---|---|
| `Cycle(list[NodeId])` | `toposort` |
| `NegativeCycle(list[NodeId])` | `bellman_ford` |
| `InvalidWeight(EdgeId)` | `dijkstra`, `astar`, `bellman_ford`, `minimum_spanning_tree` |
| `InvalidEstimate(NodeId)` | `astar` |

Algorithms return `AllocError` when their working lists cannot grow. Weights
and estimates are calculated by user callbacks, which are called only while the
algorithm runs.

## Limits

There is no graph-wide iteration protocol, compaction, subgraph, isomorphism,
max-flow, matching, or file-format support. Weakly connected components of a
directed graph, Floyd–Warshall, Prim, and petgraph's `dominators` are
candidates that need a use case. Floating-point path costs use ordinary f64
addition, so very long paths accumulate rounding.

## Acknowledgements

- API modelled on [petgraph](https://github.com/petgraph/petgraph): node and edge handles, `StableGraph`-style vacant slots, chained adjacency lists and the `algo` set. No code was copied.
- Topological order uses [Kahn's algorithm](https://doi.org/10.1145/368996.369025) (1962).
- Strongly connected components use [Tarjan's algorithm](https://doi.org/10.1137/0201010) (1972).
- Shortest paths use [Dijkstra's algorithm](https://doi.org/10.1007/BF01386390) (1959), [A\*](https://doi.org/10.1109/TSSC.1968.300136) by Hart, Nilsson and Raphael (1968), and [Bellman–Ford](https://doi.org/10.1090/qam/102435) (1958), with negative-cycle recovery as described by [cp-algorithms](https://cp-algorithms.com/graph/finding-negative-cycle-in-graph.html).
- Minimum spanning forests use [Kruskal's algorithm](https://doi.org/10.1090/S0002-9939-1956-0078686-7) (1956).
