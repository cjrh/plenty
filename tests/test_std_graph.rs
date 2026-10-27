//! `std.graph`: storage and handle contracts, traversal orders, and algorithms
//! checked against independent Rust references.
mod support;

const HEADER: &str = r#"from std.graph import DiGraph, UnGraph, GraphError, NodeId, EdgeId, Paths

def cost(weight: &f64) -> f64:
    *weight

def zero(node: NodeId) -> f64:
    0.0

def nodes(count: i64) -> Result[DiGraph[i64, f64], AllocError]:
    mut graph = DiGraph[i64, f64]()?
    for n in range(count):
        drop(graph.add_node(n)?)
    Ok(graph)

def points(count: i64) -> Result[UnGraph[i64, f64], AllocError]:
    mut graph = UnGraph[i64, f64]()?
    for n in range(count):
        drop(graph.add_node(n)?)
    Ok(graph)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}\ndef main() -> Result[(), AllocError | IoError | GraphError]:\n{body}    Ok(())\n"
    )
}

fn native(body: &str, expected: &str) {
    let source = program(body);
    let actual = support::run_file_stdout(&source);
    assert_eq!(actual, expected.trim_matches('\n'), "{source}");
}

fn execute(body: &str) -> std::process::Output {
    support::run_file(&program(body))
}

#[test]
fn nodes_and_edges_carry_owned_data() {
    native(
        r#"
mut graph = DiGraph[str, list[i64]]()?
a = graph.add_node("a")?
b = graph.add_node("b")?
e = graph.add_edge(a, b, [1, 2]?)?
print(a, b, e, graph.node_count(), graph.edge_count())?
print(*graph.node(a), graph.edge(e), graph.source(e), graph.target(e))?
label = graph.node_mut(b)
*label = "B"
weights = graph.edge_mut(e)
weights.append(3)?
print(*graph.node(b), graph.edge(e), graph.find_edge(a, b), graph.find_edge(b, a))?
loop = graph.add_edge(b, b, []?)?
print(graph.successors(b)?, graph.predecessors(b)?, graph.find_edge(b, b) == Some(loop))?
duplicate = copy(graph)?
print(duplicate.node_count(), duplicate.edge_count(), *duplicate.node(b))?
"#,
        r#"
0 1 0 2 1
a [1, 2] 0 1
B [1, 2, 3] Option[i64].Some(0) Option[i64].Nothing
[1] [0, 1] True
2 2 B
"#,
    );
}

#[test]
fn neighbors_follow_insertion_order_in_both_kinds() {
    native(
        r#"
mut directed = nodes(4)?
directed.add_edge(0, 2, 1.0)?
directed.add_edge(3, 0, 1.0)?
directed.add_edge(0, 1, 1.0)?
directed.add_edge(1, 0, 1.0)?
print(directed.successors(0)?, directed.predecessors(0)?)?
mut undirected = points(4)?
undirected.add_edge(0, 2, 1.0)?
undirected.add_edge(3, 0, 1.0)?
undirected.add_edge(0, 1, 1.0)?
loop = undirected.add_edge(0, 0, 1.0)?
print(undirected.neighbors(0)?, undirected.neighbors(3)?, undirected.endpoints(1), undirected.opposite(1, 0))?
print(undirected.find_edge(2, 0), undirected.find_edge(0, 0) == Some(loop))?
mut walk = directed.incoming(0)
while True:
    match directed.next_edge(&mut walk):
        case Some(edge):
            print("in", edge, directed.source(edge))?
        case Nothing:
            break
print(directed.next_edge(&mut walk))?
"#,
        r#"
[2, 1] [3, 1]
[2, 3, 1, 0] [0] (3, 0) 3
Option[i64].Some(0) True
in 1 3
in 3 1
Option[i64].Nothing
"#,
    );
}

#[test]
fn removal_keeps_every_other_handle_stable() {
    native(
        r#"
mut graph = DiGraph[str, f64]()?
a = graph.add_node("a")?
b = graph.add_node("b")?
c = graph.add_node("c")?
ab = graph.add_edge(a, b, 1.0)?
bc = graph.add_edge(b, c, 2.0)?
ca = graph.add_edge(c, a, 3.0)?
bb = graph.add_edge(b, b, 4.0)?
print(graph.remove_node(b), graph.remove_node(b))?
print(graph.node_count(), graph.edge_count(), graph.node_bound(), graph.edge_bound())?
print(graph.contains_node(b), graph.contains_edge(ab), graph.contains_edge(bb), graph.contains_edge(ca))?
print(*graph.node(c), *graph.edge(ca), graph.nodes()?, graph.edges()?)?
d = graph.add_node("d")?
cd = graph.add_edge(c, d, 5.0)?
print(d, cd, graph.successors(c)?, graph.remove_edge(ca), graph.remove_edge(ca), graph.successors(c)?)?
print(graph.contains_node(-1), graph.contains_edge(99))?
mut ring = points(3)?
first = ring.add_edge(0, 1, 1.0)?
second = ring.add_edge(1, 2, 2.0)?
third = ring.add_edge(2, 0, 3.0)?
print(ring.remove_edge(second), ring.neighbors(1)?, ring.neighbors(2)?, ring.edge_count())?
print(ring.remove_node(0), ring.edge_count(), ring.neighbors(1)?)?
"#,
        r#"
Option[str].Some("b") Option[str].Nothing
2 1 3 4
False False False True
c 3.0 [0, 2] [2]
3 4 [0, 3] Option[f64].Some(3.0) Option[f64].Nothing [3]
False False
Option[f64].Some(2.0) [0] [0] 2
Option[i64].Some(0) 0 []
"#,
    );
}

#[test]
fn removed_and_invalid_handles_trap() {
    for body in [
        "mut graph = nodes(2)?\ndrop(graph.remove_node(1))\nprint(*graph.node(1))?",
        "graph = nodes(2)?\nprint(*graph.node(-1))?",
        "mut graph = nodes(2)?\ngraph.add_edge(0, 5, 1.0)?",
        "mut graph = points(2)?\ne = graph.add_edge(0, 1, 1.0)?\ndrop(graph.remove_edge(e))\nprint(*graph.edge(e))?",
        "graph = points(2)?\nprint(graph.bfs(7)?)?",
    ] {
        let output = execute(body);
        assert!(!output.status.success(), "{body}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("index out of bounds"), "{body}\n{stderr}");
        assert!(
            stderr.contains("called from main.plenty"),
            "{body}\n{stderr}"
        );
    }
}

#[test]
fn checked_getters_return_nothing_for_removed_and_invalid_handles() {
    native(
        r#"
mut graph = nodes(3)?
e = graph.add_edge(0, 1, 2.5)?
match graph.get_node_mut(1):
    case Some(value):
        value += 10
    case Nothing:
        pass
match graph.get_edge_mut(e):
    case Some(weight):
        weight *= 2.0
    case Nothing:
        pass
print(graph.get_node_ref(1), graph.get_edge_ref(e))?
print(graph.get_node_ref(-1), graph.get_node_ref(3), graph.get_edge_ref(1))?
drop(graph.remove_node(1))
print(graph.get_node_ref(1), graph.get_node_mut(1).is_some(), graph.get_edge_ref(e))?
mut road = points(2)?
r = road.add_edge(0, 1, 4.0)?
drop(road.remove_edge(r))
print(road.get_node_ref(0), road.get_edge_ref(r), road.get_edge_mut(r).is_some())?
"#,
        r#"
Option[&i64].Some(11) Option[&f64].Some(5.0)
Option[&i64].Nothing Option[&i64].Nothing Option[&f64].Nothing
Option[&i64].Nothing False Option[&f64].Nothing
Option[&i64].Some(0) Option[&f64].Nothing False
"#,
    );
}

#[test]
fn breadth_and_depth_first_orders() {
    native(
        r#"
mut graph = nodes(7)?
graph.add_edge(0, 1, 1.0)?
graph.add_edge(0, 2, 1.0)?
graph.add_edge(1, 3, 1.0)?
graph.add_edge(1, 4, 1.0)?
graph.add_edge(2, 5, 1.0)?
graph.add_edge(4, 0, 1.0)?
graph.add_edge(5, 1, 1.0)?
print(graph.bfs(0)?, graph.dfs(0)?, graph.dfs(2)?, graph.bfs(6)?)?
mut ring = points(5)?
ring.add_edge(0, 1, 1.0)?
ring.add_edge(1, 2, 1.0)?
ring.add_edge(2, 3, 1.0)?
ring.add_edge(3, 0, 1.0)?
print(ring.bfs(0)?, ring.dfs(0)?, ring.connected_components()?)?
drop(ring.remove_node(1))
print(ring.connected_components()?)?
"#,
        r#"
[0, 1, 2, 3, 4, 5] [0, 1, 3, 4, 2, 5] [2, 5, 1, 3, 4, 0] [6]
[0, 1, 3, 2] [0, 1, 2, 3] [[0, 1, 3, 2], [4]]
[[0, 3, 2], [4]]
"#,
    );
}

#[test]
fn topological_order_and_cycle_reports() {
    native(
        r#"
mut graph = nodes(6)?
graph.add_edge(5, 2, 1.0)?
graph.add_edge(5, 0, 1.0)?
graph.add_edge(4, 0, 1.0)?
graph.add_edge(4, 1, 1.0)?
graph.add_edge(2, 3, 1.0)?
graph.add_edge(3, 1, 1.0)?
print(graph.toposort()?)?
back = graph.add_edge(1, 5, 1.0)?
match graph.toposort():
    case Ok(order):
        print(order)?
    case Err(error):
        print(error)?
drop(graph.remove_edge(back))
graph.add_edge(0, 0, 1.0)?
print(str.repr(graph.toposort())?)?
empty = nodes(0)?
print(empty.toposort()?, empty.strongly_connected_components()?)?
"#,
        r#"
[4, 5, 2, 0, 3, 1]
std.graph.GraphError.Cycle([2, 3, 1, 5])
Result[list[i64], AllocError | std.graph.GraphError].Err(std.graph.GraphError.Cycle([0]))
[] []
"#,
    );
}

#[test]
fn strongly_connected_components_in_reverse_topological_order() {
    native(
        r#"
mut graph = nodes(8)?
graph.add_edge(0, 1, 1.0)?
graph.add_edge(1, 2, 1.0)?
graph.add_edge(2, 0, 1.0)?
graph.add_edge(2, 3, 1.0)?
graph.add_edge(3, 4, 1.0)?
graph.add_edge(4, 3, 1.0)?
graph.add_edge(4, 5, 1.0)?
graph.add_edge(6, 6, 1.0)?
graph.add_edge(6, 7, 1.0)?
print(graph.strongly_connected_components()?)?
drop(graph.remove_node(1))
print(graph.strongly_connected_components()?)?
"#,
        r#"
[[5], [3, 4], [0, 1, 2], [7], [6]]
[[0], [5], [3, 4], [2], [7], [6]]
"#,
    );
}

#[test]
fn shortest_paths_and_their_errors() {
    native(
        r#"
mut graph = nodes(5)?
graph.add_edge(0, 1, 4.0)?
graph.add_edge(0, 2, 1.0)?
via = graph.add_edge(2, 1, 2.0)?
graph.add_edge(1, 3, 5.0)?
paths = graph.dijkstra(0, &cost)?
print(paths.distance(3), paths.path(3)?, paths.edge_to(1) == Some(via), paths.edge_to(0))?
print(paths.distance(4), paths.path(4)?, paths.path(0)?, paths.distance(99))?
print(graph.astar(0, 3, &cost, &zero)?, graph.astar(0, 4, &cost, &zero)?)?
print(graph.bellman_ford(0, &cost)?.distance(3))?
graph.add_edge(3, 4, -2.0)?
graph.add_edge(4, 1, 1.0)?
print(str.repr(graph.dijkstra(0, &cost))?)?
print(graph.bellman_ford(0, &cost)?.path(4)?)?
graph.add_edge(4, 3, -4.0)?
print(str.repr(graph.bellman_ford(0, &cost))?)?
print(str.repr(graph.bellman_ford(4, &cost))?)?
mut plain = nodes(2)?
plain.add_edge(0, 1, f64.NAN)?
print(str.repr(plain.dijkstra(0, &cost))?, str.repr(plain.bellman_ford(0, &cost))?)?
print(str.repr(plain.astar(0, 1, &cost, &(def(n: NodeId) -> f64: -1.0)))?)?
mut road = points(3)?
road.add_edge(0, 1, 2.0)?
road.add_edge(1, 2, 2.0)?
road.add_edge(2, 0, 5.0)?
print(road.dijkstra(2, &cost)?.path(0)?, str.repr(road.astar(0, 2, &cost, &zero))?)?
road.add_edge(1, 2, -1.0)?
print(str.repr(road.bellman_ford(0, &cost))?)?
"#,
        r#"
Option[f64].Some(8.0) [0, 2, 1, 3] True Option[i64].Nothing
Option[f64].Nothing [] [0] Option[f64].Nothing
Option[tuple[f64, list[i64]]].Some((8.0, [0, 2, 1, 3])) Option[tuple[f64, list[i64]]].Nothing
Option[f64].Some(8.0)
Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.InvalidWeight(4))
[0, 2, 1, 3, 4]
Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.NegativeCycle([3, 4]))
Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.NegativeCycle([3, 4]))
Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.InvalidWeight(0)) Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.InvalidWeight(0))
Result[Option[tuple[f64, list[i64]]], AllocError | std.graph.GraphError].Err(std.graph.GraphError.InvalidEstimate(0))
[2, 1, 0] Result[Option[tuple[f64, list[i64]]], AllocError | std.graph.GraphError].Ok(Option[tuple[f64, list[i64]]].Some((4.0, [0, 1, 2])))
Result[std.graph.Paths, AllocError | std.graph.GraphError].Err(std.graph.GraphError.NegativeCycle([1, 2]))
"#,
    );
}

#[test]
fn astar_guided_by_an_admissible_estimate() {
    // A 1-D corridor with a shortcut. The estimate is the remaining distance
    // to node 9; nodes off the corridor are never needed.
    native(
        r#"
mut line = nodes(12)?
for n in range(9):
    line.add_edge(n, n + 1, 1.0)?
line.add_edge(0, 10, 1.0)?
line.add_edge(10, 11, 1.0)?
line.add_edge(2, 7, 2.0)?
remaining = def(node: NodeId) -> f64:
    0.0 if node >= 10 else f64(9 - node)
print(line.astar(0, 9, &cost, &remaining)?)?
"#,
        "Option[tuple[f64, list[i64]]].Some((6.0, [0, 1, 2, 7, 8, 9]))",
    );
}

#[test]
fn minimum_spanning_forest() {
    native(
        r#"
mut graph = points(7)?
graph.add_edge(0, 1, 7.0)?
graph.add_edge(0, 3, 5.0)?
graph.add_edge(1, 2, 8.0)?
graph.add_edge(1, 3, 9.0)?
graph.add_edge(1, 4, 7.0)?
graph.add_edge(2, 4, 5.0)?
graph.add_edge(3, 4, 15.0)?
graph.add_edge(3, 5, 6.0)?
graph.add_edge(4, 5, 8.0)?
graph.add_edge(4, 6, 9.0)?
graph.add_edge(5, 6, 11.0)?
tree = graph.minimum_spanning_tree(&cost)?
mut total = 0.0
for edge in &tree:
    total += *graph.edge(*edge)
print(tree, total)?
island = graph.add_node(7)?
graph.add_edge(island, island, 1.0)?
graph.add_edge(island, 6, -3.0)?
print(graph.minimum_spanning_tree(&cost)?)?
bad = graph.add_edge(0, 2, f64.NAN)?
print(str.repr(graph.minimum_spanning_tree(&cost))?)?
"#,
        r#"
[1, 5, 7, 0, 4, 9] 39.0
[12, 1, 5, 7, 0, 4, 9]
Result[list[i64], AllocError | std.graph.GraphError].Err(std.graph.GraphError.InvalidWeight(13))
"#,
    );
}

#[test]
fn kind_specific_algorithms_are_rejected_on_the_other_kind() {
    let toposort =
        support::check_file_error(&program("graph = points(2)?\nprint(graph.toposort()?)?"));
    assert!(toposort.contains("toposort"), "{toposort}");
    let tree = support::check_file_error(&program(
        "graph = nodes(2)?\nprint(graph.minimum_spanning_tree(&cost)?)?",
    ));
    assert!(tree.contains("minimum_spanning_tree"), "{tree}");
    let unit = support::check_file_error(
        "from std.graph import DiGraph\n\ndef main() -> Result[(), AllocError]:\n    graph = DiGraph[i64, ()]()?\n    Ok(())\n",
    );
    assert!(unit.contains("unit type arguments"), "{unit}");
}

#[test]
fn deep_graphs_do_not_recurse() {
    native(
        r#"
mut chain = nodes(0)?
chain.reserve(200000, 200000)?
mut previous = chain.add_node(0)?
for n in range(1, 200000):
    next = chain.add_node(n)?
    chain.add_edge(previous, next, 1.0)?
    previous = next
print(len(chain.dfs(0)?), len(chain.bfs(0)?), len(chain.toposort()?))?
print(len(chain.strongly_connected_components()?), chain.dijkstra(0, &cost)?.distance(previous))?
chain.add_edge(previous, 0, 1.0)?
print(len(chain.strongly_connected_components()?), len(chain.dfs(previous)?))?
"#,
        "200000 200000 200000\n200000 Option[f64].Some(199999.0)\n1 200000",
    );
}

#[test]
fn reserved_growth_lookups_and_walks_allocate_nothing() {
    native(
        r#"
mut graph = DiGraph[i64, f64]()?
graph.reserve(4, 4)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
a = graph.add_node(1).unwrap()
b = graph.add_node(2).unwrap()
e = graph.add_edge(a, b, 0.5).unwrap()
graph.add_edge(b, a, 1.5).unwrap()
mut walk = graph.outgoing(a)
found = graph.next_edge(&mut walk)
weight = *graph.edge(e)
removed = graph.remove_edge(e)
gone = graph.remove_node(b)
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(found, weight, removed, gone, graph.edge_count())?
"#,
        "Option[i64].Some(0) 0.5 Option[f64].Some(0.5) Option[i64].Some(2) 0",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn growth_failure_preserves_the_graph() {
    native(
        r#"
mut graph = UnGraph[str, str]()?
a = graph.add_node("a")?
mut failed = False
while not failed:
    print("__test_fail_allocations_after_0__")?
    result = graph.add_edge(a, a, "loop")
    print("__test_restore_allocations__")?
    match result:
        case Ok(_):
            pass
        case Err(error):
            print(error)?
            failed = True
count = graph.edge_count()
print(count == graph.edge_bound(), len(graph.neighbors(a)?) == count)?
"#,
        "AllocError.OutOfMemory\nTrue True",
    );
}

/// A deterministic pseudo-random weighted digraph, checked against
/// Floyd–Warshall distances, a Kruskal total over the symmetric closure, and
/// a reachability-based component partition.
#[test]
fn random_graphs_agree_with_reference_algorithms() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    for _ in 0..6 {
        let n = 12 + next(10) as usize;
        let m = 2 * n + next(2 * n as u64) as usize;
        let edges = (0..m)
            .map(|_| {
                (
                    next(n as u64) as usize,
                    next(n as u64) as usize,
                    next(20) as f64,
                )
            })
            .collect::<Vec<_>>();

        let mut dist = vec![vec![f64::INFINITY; n]; n];
        for (i, row) in dist.iter_mut().enumerate() {
            row[i] = 0.0;
        }
        for &(a, b, w) in &edges {
            dist[a][b] = dist[a][b].min(w);
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    let via = dist[i][k] + dist[k][j];
                    if via < dist[i][j] {
                        dist[i][j] = via;
                    }
                }
            }
        }
        let expected_distances = (0..n)
            .map(|j| {
                if dist[0][j].is_finite() {
                    format!("{:.1}", dist[0][j])
                } else {
                    "-".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let mut parent = (0..n).collect::<Vec<_>>();
        fn root(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                x = parent[x];
            }
            x
        }
        let mut sorted = edges.clone();
        sorted.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut tree_total = 0.0;
        for &(a, b, w) in &sorted {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            if ra != rb {
                parent[ra] = rb;
                tree_total += w;
            }
        }

        let mut classes = (0..n)
            .map(|i| {
                (0..n)
                    .filter(|&j| dist[i][j].is_finite() && dist[j][i].is_finite())
                    .min()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        classes.sort();
        classes.dedup();

        let additions = edges
            .iter()
            .map(|(a, b, w)| {
                format!(
                    "directed.add_edge({a}, {b}, {w:.1})?\nundirected.add_edge({a}, {b}, {w:.1})?\n"
                )
            })
            .collect::<String>();
        let body = format!(
            r#"
mut directed = nodes({n})?
mut undirected = points({n})?
{additions}paths = directed.dijkstra(0, &cost)?
checked = directed.bellman_ford(0, &cost)?
for node in range({n}):
    if paths.distance(node) != checked.distance(node):
        print("mismatch", node)?
    match paths.distance(node):
        case Some(value):
            print(value)?
        case Nothing:
            print("-")?
mut total = 0.0
tree = undirected.minimum_spanning_tree(&cost)?
for edge in &tree:
    total += *undirected.edge(*edge)
print(total, len(directed.strongly_connected_components()?))?
"#
        );
        let source = program(&body);
        let actual = support::run_file_stdout(&source);
        let expected = format!("{expected_distances}\n{tree_total:.1} {}", classes.len());
        assert_eq!(actual, expected, "{source}");
    }
}
