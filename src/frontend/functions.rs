//! The whole-program function table of lowering.
//!
//! Each function name is hashed once, into a [`Symbol`]; its signature,
//! declaration and returned fields are found by that index. Separate
//! name-keyed tables cost a string hash, a key comparison and a cache miss
//! per probe once a program outgrows the caches (issues #57 and #426).
use super::generics::Declared;
use super::*;

/// A function name's index in [`Functions`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct Symbol(u32);

#[derive(Default)]
struct Entry {
    sig: Option<Arc<FnSig>>,
    declared: Option<Declared>,
}

#[derive(Default)]
pub(super) struct Functions {
    symbols: HashMap<String, Symbol>,
    entries: Vec<Entry>,
    /// Few functions return borrowed fields, so these stay out of `entries`.
    returned_fields: HashMap<Symbol, Vec<usize>>,
    /// Declared functions, counted for generated names.
    declared: usize,
}

impl Functions {
    pub(super) fn with_capacity(functions: usize) -> Self {
        Self {
            symbols: HashMap::with_capacity_and_hasher(functions, Default::default()),
            entries: Vec::with_capacity(functions),
            ..Self::default()
        }
    }

    pub(super) fn symbol(&self, name: &str) -> Option<Symbol> {
        self.symbols.get(name).copied()
    }

    /// The symbol of `name`, which is added if it is new.
    pub(super) fn intern(&mut self, name: &str) -> Symbol {
        if let Some(symbol) = self.symbol(name) {
            return symbol;
        }
        self.add(name.to_owned())
    }

    fn add(&mut self, name: String) -> Symbol {
        let symbol = Symbol(u32::try_from(self.entries.len()).expect("fewer than 2^32 functions"));
        self.entries.push(Entry::default());
        self.symbols.insert(name, symbol);
        symbol
    }

    pub(super) fn get(&self, name: &str) -> Option<&Arc<FnSig>> {
        self.symbol(name).and_then(|symbol| self.sig(symbol))
    }

    pub(super) fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub(super) fn get_mut(&mut self, name: &str) -> Option<&mut Arc<FnSig>> {
        let symbol = self.symbol(name)?;
        self.entries[symbol.0 as usize].sig.as_mut()
    }

    pub(super) fn insert(&mut self, name: String, sig: Arc<FnSig>) {
        let symbol = match self.symbol(&name) {
            Some(symbol) => symbol,
            None => self.add(name),
        };
        self.entries[symbol.0 as usize].sig = Some(sig);
    }

    pub(super) fn sig(&self, symbol: Symbol) -> Option<&Arc<FnSig>> {
        self.entries[symbol.0 as usize].sig.as_ref()
    }

    /// Records `sig` for a function without one; returns whether it had none.
    pub(super) fn register(&mut self, symbol: Symbol, sig: &Arc<FnSig>) -> bool {
        let entry = &mut self.entries[symbol.0 as usize].sig;
        let vacant = entry.is_none();
        if vacant {
            *entry = Some(Arc::clone(sig));
        }
        vacant
    }

    pub(super) fn declared(&self, name: &str) -> Option<&Declared> {
        self.symbol(name)
            .and_then(|symbol| self.declared_at(symbol))
    }

    pub(super) fn declared_at(&self, symbol: Symbol) -> Option<&Declared> {
        self.entries[symbol.0 as usize].declared.as_ref()
    }

    pub(super) fn declared_mut(&mut self, symbol: Symbol) -> Option<&mut Declared> {
        self.entries[symbol.0 as usize].declared.as_mut()
    }

    pub(super) fn declare(&mut self, symbol: Symbol, declared: Declared) {
        let entry = &mut self.entries[symbol.0 as usize].declared;
        self.declared += usize::from(entry.is_none());
        *entry = Some(declared);
    }

    pub(super) fn lowered(&self, name: &str) -> bool {
        self.declared(name)
            .expect("a lowered function is declared")
            .lowered
    }

    /// Marks a function lowered and releases its syntax tree. A function with
    /// a generator parameter keeps its tree: each frame specialization copies
    /// its declaration.
    pub(super) fn complete(&mut self, name: &str, sig: &FnSig) {
        let symbol = self.symbol(name).expect("a lowered function is declared");
        let declared = self
            .declared_mut(symbol)
            .expect("a lowered function is declared");
        declared.lowered = true;
        if !sig.inputs.iter().any(|(_, t)| t.unresolved_generator()) {
            declared.function = None;
        }
    }

    /// How many functions are declared.
    pub(super) fn declared_len(&self) -> usize {
        self.declared
    }

    pub(super) fn returned_fields(&self, name: &str) -> Option<&Vec<usize>> {
        self.returned_fields.get(&self.symbol(name)?)
    }

    pub(super) fn set_returned_fields(&mut self, symbol: Symbol, fields: Vec<usize>) {
        self.returned_fields.insert(symbol, fields);
    }
}

impl<Q: AsRef<str> + ?Sized> std::ops::Index<&Q> for Functions {
    type Output = Arc<FnSig>;

    fn index(&self, name: &Q) -> &Arc<FnSig> {
        self.get(name.as_ref())
            .expect("a function with a registered signature")
    }
}
