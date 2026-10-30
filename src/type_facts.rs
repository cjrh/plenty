//! Properties of finite type graphs. Leaf elimination identifies both cycles and
//! their owners; ownership facts are unions over reachable storage, not recursive
//! calls that assume a declaration has already finished.
use crate::hash::HashMap;
use crate::op::Ty;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub affine: bool,
    pub copyable: bool,
    pub destructor: bool,
    pub recursive: bool,
    /// A cycle with no heap boundary: the value would contain itself inline.
    pub infinite: bool,
    pub complete: bool,
    pub depth: usize,
    pub reflexive: bool,
    pub closure: bool,
    pub concurrent_handle: bool,
    /// Reaches an `Atomic`, which has no equality and no formatting: its
    /// value is only what one `load` observed.
    pub atomic: bool,
    /// Reaches a `Shared` owner, which has no equality or formatting:
    /// several owners observe one value.
    pub shared: bool,
}

impl Ty {
    pub fn facts(&self) -> Facts {
        if !matches!(
            self,
            Self::Class(_)
                | Self::Enum(_)
                | Self::List(_)
                | Self::Set(_)
                | Self::Dict(..)
                | Self::Box(_)
                | Self::Shared(_)
        ) {
            let resource = self.is_resource() || matches!(self, Self::Task(_));
            return Facts {
                affine: resource || matches!(self, Self::Atomic(_)),
                copyable: !resource
                    && !matches!(self, Self::Ref(..) | Self::KeyView(_) | Self::Atomic(_)),
                destructor: resource,
                complete: true,
                reflexive: !self.is_float() && !matches!(self, Self::Clist(_)),
                closure: matches!(self, Self::Closure(_) | Self::BoxedCallable(_)),
                concurrent_handle: self.is_concurrent_handle(),
                atomic: matches!(self, Self::Atomic(_)),
                ..Facts::default()
            };
        }
        let cached = match self {
            Self::Class(t) => t.cached_facts(),
            Self::Enum(t) => t.cached_facts(),
            Self::Dict(k, v) => return Self::owner_facts(&[k, v]),
            Self::List(t) | Self::Set(t) | Self::Box(t) => return Self::owner_facts(&[t]),
            Self::Shared(t) => {
                let mut facts = Self::owner_facts(&[t]);
                facts.copyable = false;
                facts.shared = true;
                return facts;
            }
            _ => None,
        };
        if let Some(facts) = cached {
            return facts;
        }
        let facts = self.walk();
        if facts.complete {
            match self {
                Self::Class(t) => t.cache_facts(facts),
                Self::Enum(t) => t.cache_facts(facts),
                _ => {}
            }
        }
        facts
    }
    /// A leaf that owns a runtime resource: affine, not copyable, and dropped
    /// by a destructor wherever a walk reaches it. `Task` is deliberately
    /// absent: only `facts` on a task itself treats it as a resource, and a
    /// walk never does (see `owner_facts`). Every concurrent handle is a
    /// resource.
    fn is_resource(&self) -> bool {
        matches!(
            self,
            Self::File
                | Self::FsReadDir
                | Self::FsDirEntry
                | Self::FsMetadata
                | Self::ByteStorage
                | Self::Clist(_)
                | Self::Generator(_)
                | Self::Closure(_)
                | Self::BoxedCallable(_)
        ) || self.is_concurrent_handle()
    }
    fn is_concurrent_handle(&self) -> bool {
        matches!(
            self,
            Self::Channel(..) | Self::CancellationToken | Self::Executor | Self::Future(_)
        )
    }
    /// Facts of a collection or box, combined from those of its contents
    /// instead of walking the graph again; nominal contents answer from their
    /// cache. This equals a walk from the owner: each fact is a union over
    /// reachable nodes or a property of the cycles they reach, and the owner
    /// adds one node that reaches a cycle only through its contents.
    fn owner_facts(contents: &[&Self]) -> Facts {
        let mut facts = Facts {
            affine: true,
            copyable: true,
            complete: true,
            reflexive: true,
            ..Facts::default()
        };
        for ty in contents {
            let part = match ty {
                Self::Class(_)
                | Self::Enum(_)
                | Self::List(_)
                | Self::Set(_)
                | Self::Dict(..)
                | Self::Box(_)
                | Self::Shared(_) => ty.facts(),
                // A walk weighs a reachable leaf and never treats a task as a
                // resource, so its one-node walk differs from `facts`.
                _ => ty.walk(),
            };
            facts.copyable &= part.copyable;
            facts.destructor |= part.destructor;
            facts.recursive |= part.recursive;
            facts.infinite |= part.infinite;
            facts.complete &= part.complete;
            facts.depth = facts.depth.max(part.depth);
            facts.reflexive &= part.reflexive;
            facts.closure |= part.closure;
            facts.concurrent_handle |= part.concurrent_handle;
            facts.atomic |= part.atomic;
            facts.shared |= part.shared;
        }
        // An owner that reaches a cycle is a heap boundary, which a walk cuts
        // before measuring depth.
        facts.depth = if facts.recursive { 1 } else { 1 + facts.depth };
        facts
    }
    fn walk(&self) -> Facts {
        self.walk_from(false)
    }
    /// Facts of what `copy()` duplicates: owned parts are copied, and each
    /// nested `Shared` owner gains another owner as `share()` would, so content
    /// behind a handle is neither copied nor reached. `copyable` says whether
    /// `copy()` accepts the type and `recursive` whether owned parts recurse.
    /// A `Shared` value itself is copyable here; `copy()` still rejects it at
    /// the top level, where `share()` states the intent. See
    /// book/src/design/facilities/01-builtins/shared.md.
    pub fn copy_facts(&self) -> Facts {
        let facts = self.facts();
        if !facts.shared {
            return facts;
        }
        self.walk_from(true)
    }
    #[expect(
        clippy::mutable_key_type,
        reason = "nominal identities exclude definition cells"
    )]
    fn walk_from(&self, share_handles: bool) -> Facts {
        let mut facts = Facts {
            copyable: true,
            complete: true,
            reflexive: true,
            ..Facts::default()
        };
        let mut nodes = vec![self.clone()];
        let mut ids = HashMap::from_iter([(self.clone(), 0)]);
        let mut parents: Vec<Vec<usize>> = vec![vec![]];
        let mut degree = Vec::new();
        let mut depths = Vec::new();
        let mut weights = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            facts.closure |= matches!(nodes[i], Self::Closure(_) | Self::BoxedCallable(_));
            facts.concurrent_handle |= nodes[i].is_concurrent_handle();
            facts.atomic |= matches!(nodes[i], Self::Atomic(_));
            facts.shared |= matches!(nodes[i], Self::Shared(_));
            facts.reflexive &= !nodes[i].is_float() && !matches!(nodes[i], Self::Clist(_));
            let weight = match &nodes[i] {
                Self::Enum(t) => t.try_get().map_or(0, |definition| {
                    if t.propagatable() {
                        1
                    } else {
                        definition.variants.len().next_power_of_two().ilog2().max(1) as usize
                    }
                }),
                Self::Class(t) => usize::from(t.try_get().is_some()),
                Self::List(_)
                | Self::Set(_)
                | Self::Dict(..)
                | Self::Box(_)
                | Self::Shared(_)
                | Self::Callable(_) => 1,
                Self::BoxedCallable(_) => 1,
                Self::Closure(t) => t.depth,
                Self::Generator(t) => 1 + t.element.layout_depth(),
                // A reference's high word holds a tag shift of at most 64,
                // which the tags of an enclosing Option or Result stack on.
                Self::Ref(..) => 7,
                _ => 0,
            };
            weights.push(weight);
            depths.push(weight);
            let children = match &nodes[i] {
                Self::Class(t) => match t.try_get() {
                    Some(t) => {
                        facts.affine |= !t.copy;
                        facts.destructor |= t.destructor.is_some();
                        facts.copyable &= t.destructor.is_none();
                        t.fields.iter().map(|(_, t)| t.clone()).collect()
                    }
                    _ => {
                        facts.affine = true;
                        facts.complete = false;
                        vec![]
                    }
                },
                Self::Enum(t) => match t.try_get() {
                    Some(t) => t
                        .variants
                        .iter()
                        .flat_map(|v| v.fields.iter().cloned())
                        .collect(),
                    _ => {
                        facts.complete = false;
                        vec![]
                    }
                },
                Self::List(t) | Self::Set(t) | Self::Box(t) => {
                    facts.affine = true;
                    vec![(**t).clone()]
                }
                Self::Shared(_) if share_handles => {
                    facts.affine = true;
                    vec![]
                }
                Self::Shared(t) => {
                    facts.affine = true;
                    facts.copyable = false;
                    vec![(**t).clone()]
                }
                Self::Dict(k, v) => {
                    facts.affine = true;
                    vec![(**k).clone(), (**v).clone()]
                }
                node if node.is_resource() => {
                    facts.affine = true;
                    facts.copyable = false;
                    facts.destructor = true;
                    vec![]
                }
                Self::Ref(..) => {
                    facts.copyable = false;
                    vec![]
                }
                // One owner per atomic: copying would split its updates.
                Self::Atomic(_) => {
                    facts.affine = true;
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
            // Boxes and collections keep their contents on the heap, which
            // breaks physical layout cycles. Cutting these boundaries still
            // counts every surrounding inline sum tag, including a long
            // Option[Option[...Node]] chain outside the cycle.
            let boundaries: Vec<_> = nodes
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    degree[i] != 0
                        && matches!(
                            t,
                            Self::Box(_)
                                | Self::Shared(_)
                                | Self::List(_)
                                | Self::Set(_)
                                | Self::Dict(..)
                        )
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
            facts.infinite = degree.iter().any(|&n| n != 0);
        }
        facts.depth = depths[0];
        // Deep copying still uses a native recursive traversal. Do not expose it
        // for unbounded recursive values until an iterative implementation exists.
        facts.copyable &= !facts.recursive;
        facts.affine |= facts.recursive;
        facts
    }
    pub fn recursive_data(&self) -> bool {
        self.facts().recursive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nominal::Types;
    use crate::record::ClassType;
    use crate::sum::{EnumType, Variant, option};
    use std::sync::Arc;
    use std::sync::OnceLock;

    fn list(t: &Ty) -> Ty {
        Ty::List(Arc::new(t.clone()))
    }
    fn set(t: &Ty) -> Ty {
        Ty::Set(Arc::new(t.clone()))
    }
    fn boxed(t: &Ty) -> Ty {
        Ty::Box(Arc::new(t.clone()))
    }
    fn dict(k: &Ty, v: &Ty) -> Ty {
        Ty::Dict(Arc::new(k.clone()), Arc::new(v.clone()))
    }
    fn class(name: &str, fields: Vec<Ty>, destructor: bool) -> ClassType {
        ClassType {
            name: name.into(),
            fields: fields
                .into_iter()
                .enumerate()
                .map(|(i, t)| (format!("f{i}"), t))
                .collect(),
            destructor: destructor.then(|| "__del__".into()),
            fallible_init: false,
            copy: false,
            forwards_operators: false,
            redacted: false,
            bytes: OnceLock::new(),
            managed: OnceLock::new(),
        }
    }
    fn enumeration(name: &str, variants: Vec<Vec<Ty>>) -> EnumType {
        EnumType {
            facts: OnceLock::new(),
            restricted_storage: false,
            redacted: false,
            name: name.into(),
            variants: variants
                .into_iter()
                .enumerate()
                .map(|(i, fields)| Variant {
                    name: format!("V{i}"),
                    fields,
                })
                .collect(),
            managed: true,
            managed_fields: OnceLock::new(),
            inline_range: false,
            payload_bytes: OnceLock::new(),
            by_name: OnceLock::new(),
            storage: OnceLock::new(),
        }
    }

    fn leaves() -> Vec<Ty> {
        let signature = crate::op::CallableSig {
            inputs: vec![Ty::I64],
            output: Some(Ty::Bool),
        };
        let closure = crate::closure::ClosureType::new(
            "environment".into(),
            None,
            signature.clone(),
            vec![("captured".into(), list(&Ty::I64))],
            vec![false],
            false,
        )
        .unwrap();
        vec![
            Ty::I64,
            Ty::F64,
            Ty::Str,
            Ty::Bool,
            Ty::Unit,
            Ty::File,
            Ty::Executor,
            Ty::CancellationToken,
            Ty::ForeignPtr("handle".into()),
            Ty::Range(Arc::new(Ty::I64)),
            Ty::Ref(Arc::new(Ty::I64), false),
            Ty::Channel(Arc::new(Ty::I64), true),
            Ty::Future(Arc::new(Ty::I64)),
            Ty::Callable(Arc::new(signature.clone())),
            Ty::BoxedCallable(Arc::new(signature)),
            Ty::Closure(Arc::new(closure)),
            Ty::Generator(Arc::new(crate::generator::GeneratorType {
                element: list(&Ty::I64),
                name: Some("frame".into()),
                slots: OnceLock::new(),
                layout: OnceLock::new(),
            })),
            Ty::Task(Arc::new(crate::threading::Task {
                adapter: "adapter".into(),
                worker: "worker".into(),
                closure: None,
                inputs: vec![],
                output: Ty::I64,
            })),
        ]
    }

    /// Plain, destructor-bearing, recursive, mutually recursive, infinite, and
    /// incomplete declarations. `Pending` is reserved but never defined.
    fn nominals(owner: &Arc<Types>) -> Vec<Ty> {
        let leaf = leaves();
        let point = owner.class("Point");
        point.define(class("Point", vec![Ty::I64, Ty::I64], false));
        let point = Ty::Class(point);
        let measure = owner.class("Measure");
        measure.define(class("Measure", vec![Ty::F64], false));
        let resource = owner.class("Resource");
        resource.define(class("Resource", vec![Ty::Str], true));
        let worker = owner.class("Worker");
        worker.define(class("Worker", leaf, false));
        let outer = owner.class("Outer");
        outer.define(class(
            "Outer",
            vec![point.clone(), list(&dict(&Ty::Str, &point))],
            false,
        ));
        let node = owner.class("Node");
        node.define(class(
            "Node",
            vec![list(&option(Ty::Class(node.clone())))],
            false,
        ));
        let chain = owner.class("Chain");
        chain.define(class(
            "Chain",
            vec![option(boxed(&Ty::Class(chain.clone())))],
            false,
        ));
        let tree = owner.class("Tree");
        tree.define(class("Tree", vec![set(&Ty::Class(tree.clone()))], true));
        let (left, right) = (owner.class("Left"), owner.class("Right"));
        left.define(class("Left", vec![list(&Ty::Class(right.clone()))], false));
        right.define(class(
            "Right",
            vec![dict(&Ty::Str, &Ty::Class(left.clone()))],
            false,
        ));
        let inline = owner.class("Inline");
        inline.define(class(
            "Inline",
            vec![option(Ty::Class(inline.clone()))],
            false,
        ));
        let pending = Ty::Class(owner.class("Pending"));
        let partial = owner.class("Partial");
        partial.define(class("Partial", vec![list(&pending)], false));
        let shape = owner.enumeration("Shape");
        shape.define(enumeration(
            "Shape",
            vec![vec![], vec![Ty::F64], vec![list(&point)]],
        ));
        let expression = owner.enumeration("Expression");
        let operand = boxed(&Ty::Enum(expression.clone()));
        expression.define(enumeration(
            "Expression",
            vec![vec![Ty::I64], vec![operand.clone(), operand]],
        ));
        vec![
            option(point.clone()),
            option(Ty::Class(node.clone())),
            point,
            Ty::Class(measure),
            Ty::Class(resource),
            Ty::Class(worker),
            Ty::Class(outer),
            Ty::Class(node),
            Ty::Class(chain),
            Ty::Class(tree),
            Ty::Class(left),
            Ty::Class(right),
            Ty::Class(inline),
            pending,
            Ty::Class(partial),
            Ty::Enum(shape),
            Ty::Enum(expression),
        ]
    }

    /// Each type under every owner kind, and as a dict key and value beside
    /// other types, so combined facts come from two different graphs.
    fn owners(types: &[Ty], strides: &[usize]) -> Vec<Ty> {
        let mut out = Vec::new();
        for (i, t) in types.iter().enumerate() {
            out.extend([list(t), set(t), boxed(t)]);
            for stride in strides {
                out.push(dict(t, &types[(i + stride) % types.len()]));
            }
        }
        out
    }

    #[test]
    fn owner_facts_equal_a_walk_from_the_owner() {
        let owner = Arc::new(Types::default());
        let nominal = nominals(&owner);
        for ty in &nominal {
            assert_eq!(ty.facts(), ty.walk(), "{ty:?}");
        }
        let mut level = leaves();
        level.extend(nominal);
        let every_pair: Vec<usize> = (0..level.len()).collect();
        // Three levels reach shapes such as list[dict[str, set[Node]]].
        for strides in [&every_pair[..], &[1, 5], &[3]] {
            level = owners(&level, strides);
            for ty in &level {
                assert_eq!(ty.facts(), ty.walk(), "{ty:?}");
            }
        }
        assert!(level.len() > 10_000);
    }

    #[test]
    fn nested_and_recursive_owner_facts() {
        let owner = Arc::new(Types::default());
        let nominal = nominals(&owner);
        let find = |name: &str| {
            nominal
                .iter()
                .find(|t| format!("{t:?}") == format!("Class({name})"))
                .unwrap()
        };
        let plain = Facts {
            affine: true,
            copyable: true,
            complete: true,
            reflexive: true,
            ..Facts::default()
        };
        assert_eq!(list(&Ty::I64).facts(), Facts { depth: 1, ..plain });
        assert_eq!(
            set(&Ty::F64).facts(),
            Facts {
                depth: 1,
                reflexive: false,
                ..plain
            }
        );
        assert_eq!(
            dict(&Ty::Str, &list(&list(find("Point")))).facts(),
            Facts { depth: 4, ..plain }
        );
        assert_eq!(
            list(&dict(&Ty::Str, find("Resource"))).facts(),
            Facts {
                depth: 3,
                copyable: false,
                destructor: true,
                ..plain
            }
        );
        // A recursive owner is a heap boundary: its depth is its own slot.
        let recursive = Facts {
            depth: 1,
            copyable: false,
            recursive: true,
            ..plain
        };
        assert_eq!(list(find("Node")).facts(), recursive);
        assert_eq!(boxed(&set(find("Chain"))).facts(), recursive);
        assert_eq!(dict(find("Point"), find("Left")).facts(), recursive);
        assert_eq!(dict(&Ty::Str, find("Right")).facts(), recursive);
        assert_eq!(
            list(find("Tree")).facts(),
            Facts {
                destructor: true,
                ..recursive
            }
        );
        assert_eq!(
            list(find("Inline")).facts(),
            Facts {
                infinite: true,
                ..recursive
            }
        );
        assert_eq!(
            list(find("Partial")).facts(),
            Facts {
                depth: 3,
                complete: false,
                ..plain
            }
        );
    }

    #[test]
    fn owner_facts_follow_later_definitions() {
        let owner = Arc::new(Types::default());
        let (left, right) = (owner.class("Left"), owner.class("Right"));
        left.define(class("Left", vec![list(&Ty::Class(right.clone()))], false));
        let ty = dict(&Ty::Str, &list(&Ty::Class(left.clone())));
        let before = ty.facts();
        assert!(!before.complete && !before.recursive && before.copyable);
        assert_eq!(before, ty.walk());
        right.define(class("Right", vec![set(&Ty::Class(left.clone()))], true));
        let after = ty.facts();
        assert!(after.complete && after.recursive && after.destructor && !after.copyable);
        assert_eq!(after, ty.walk());
    }
}
