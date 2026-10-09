//! Properties of finite type graphs. Leaf elimination identifies both cycles and
//! their owners; ownership facts are unions over reachable storage, not recursive
//! calls that assume a declaration has already finished.
use crate::op::Ty;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    pub affine: bool,
    pub copyable: bool,
    pub destructor: bool,
    pub recursive: bool,
    pub complete: bool,
    pub depth: usize,
    pub reflexive: bool,
    pub closure: bool,
    pub concurrent_handle: bool,
}

impl Ty {
    #[expect(
        clippy::mutable_key_type,
        reason = "nominal identities exclude definition cells"
    )]
    pub fn facts(&self) -> Facts {
        if !matches!(
            self,
            Self::Class(_) | Self::Enum(_) | Self::List(_) | Self::Set(_) | Self::Dict(..)
        ) {
            let resource = matches!(
                self,
                Self::File
                    | Self::Generator(_)
                    | Self::Closure(_)
                    | Self::Task(_)
                    | Self::Channel(..)
                    | Self::CancellationToken
                    | Self::Executor
                    | Self::Future(_)
            );
            return Facts {
                affine: resource,
                copyable: !resource && !matches!(self, Self::Ref(..)),
                destructor: resource,
                complete: true,
                reflexive: !self.is_float(),
                closure: matches!(self, Self::Closure(_)),
                concurrent_handle: matches!(
                    self,
                    Self::Channel(..) | Self::CancellationToken | Self::Executor | Self::Future(_)
                ),
                ..Facts::default()
            };
        }
        let cached = match self {
            Self::Class(t) => t.cached_facts(),
            Self::Enum(t) => t.cached_facts(),
            _ => None,
        };
        if let Some(facts) = cached {
            return facts;
        }
        let mut facts = Facts {
            copyable: true,
            complete: true,
            reflexive: true,
            ..Facts::default()
        };
        let mut nodes = vec![self.clone()];
        let mut ids = HashMap::from([(self.clone(), 0)]);
        let mut parents: Vec<Vec<usize>> = vec![vec![]];
        let mut degree = Vec::new();
        let mut depths = Vec::new();
        let mut weights = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            facts.closure |= matches!(nodes[i], Self::Closure(_));
            facts.concurrent_handle |= matches!(
                nodes[i],
                Self::Channel(..) | Self::CancellationToken | Self::Executor | Self::Future(_)
            );
            facts.reflexive &= !nodes[i].is_float();
            let weight = match &nodes[i] {
                Self::Enum(t) => t.try_get().map_or(0, |definition| {
                    if t.inline() && !t.propagatable() {
                        definition.variants.len().next_power_of_two().ilog2().max(1) as usize
                    } else {
                        1
                    }
                }),
                Self::Class(t) => usize::from(t.try_get().is_some()),
                Self::List(_) | Self::Set(_) | Self::Dict(..) | Self::Callable(_) => 1,
                Self::Closure(t) => t.depth,
                Self::Generator(t) => 1 + t.element.layout_depth(),
                _ => 0,
            };
            weights.push(weight);
            depths.push(weight);
            let children = match &nodes[i] {
                Self::Class(t) => {
                    facts.affine = true;
                    if let Some(t) = t.try_get() {
                        facts.destructor |= t.destructor.is_some();
                        facts.copyable &= t.destructor.is_none();
                        t.fields.iter().map(|(_, t)| t.clone()).collect()
                    } else {
                        facts.complete = false;
                        vec![]
                    }
                }
                Self::Enum(t) => {
                    if let Some(t) = t.try_get() {
                        t.variants
                            .iter()
                            .flat_map(|v| v.fields.iter().cloned())
                            .collect()
                    } else {
                        facts.complete = false;
                        vec![]
                    }
                }
                Self::List(t) | Self::Set(t) => {
                    facts.affine = true;
                    vec![(**t).clone()]
                }
                Self::Dict(k, v) => {
                    facts.affine = true;
                    vec![(**k).clone(), (**v).clone()]
                }
                Self::File
                | Self::Closure(_)
                | Self::Generator(_)
                | Self::Channel(..)
                | Self::CancellationToken
                | Self::Executor
                | Self::Future(_) => {
                    facts.affine = true;
                    facts.copyable = false;
                    facts.destructor = true;
                    vec![]
                }
                Self::Ref(..) => {
                    facts.copyable = false;
                    vec![]
                }
                _ => vec![],
            };
            degree.push(children.len());
            for child in children {
                let id = *ids.entry(child.clone()).or_insert_with(|| {
                    let id = nodes.len();
                    nodes.push(child);
                    parents.push(vec![]);
                    id
                });
                parents[id].push(i);
            }
            i += 1;
        }
        let mut leaves: VecDeque<_> = degree
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect();
        while let Some(i) = leaves.pop_front() {
            for &parent in &parents[i] {
                depths[parent] = depths[parent].max(weights[parent] + depths[i]);
                degree[parent] -= 1;
                if degree[parent] == 0 {
                    leaves.push_back(parent);
                }
            }
        }
        facts.recursive = degree[0] != 0;
        if facts.recursive {
            // Heap nominal records break physical layout cycles. Cutting these
            // boundaries still counts every surrounding inline sum tag, including
            // a long Option[Option[...Node]] chain outside the cycle.
            let boundaries: Vec<_> = nodes
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    degree[i] != 0
                        && (matches!(t, Self::Class(_))
                            || matches!(t, Self::Enum(e) if !e.inline() && !e.tuple()))
                })
                .collect();
            degree.fill(0);
            for ps in &parents {
                for &p in ps {
                    if !boundaries[p] {
                        degree[p] += 1;
                    }
                }
            }
            depths.clone_from(&weights);
            let mut leaves: VecDeque<_> = degree
                .iter()
                .enumerate()
                .filter_map(|(i, &n)| (n == 0).then_some(i))
                .collect();
            while let Some(i) = leaves.pop_front() {
                for &p in &parents[i] {
                    if boundaries[p] {
                        continue;
                    }
                    depths[p] = depths[p].max(weights[p] + depths[i]);
                    degree[p] -= 1;
                    if degree[p] == 0 {
                        leaves.push_back(p);
                    }
                }
            }
        }
        facts.depth = depths[0];
        // Deep copying still uses a native recursive traversal. Do not expose it
        // for unbounded recursive values until an iterative implementation exists.
        facts.copyable &= !facts.recursive;
        facts.affine |= facts.recursive;
        if facts.complete {
            match self {
                Self::Class(t) => t.cache_facts(facts),
                Self::Enum(t) => t.cache_facts(facts),
                _ => {}
            }
        }
        facts
    }
    pub fn recursive_data(&self) -> bool {
        self.facts().recursive
    }
}
