//! The book's highlight.js grammar (`book/theme/plenty-highlight.js`) must
//! classify the words the compiler reserves or predefines (issue #154).

use std::collections::BTreeSet;

const GRAMMAR: &str = include_str!("../book/theme/plenty-highlight.js");
const FRONTEND: &str = include_str!("../src/frontend.rs");
const ENUMS: &str = include_str!("../src/frontend/enums.rs");

/// The text from `start` to the end of that item: the next line holding only `}`.
fn item<'a>(source: &'a str, start: &str) -> &'a str {
    let begin = source
        .find(start)
        .unwrap_or_else(|| panic!("`{start}` not found; update this test"));
    let rest = &source[begin..];
    &rest[..rest.find("\n}\n").expect("item end")]
}

fn quoted(text: &str) -> BTreeSet<&str> {
    text.split('"').skip(1).step_by(2).collect()
}

/// A grammar list, written as `const NAME = [ "word", ... ];`.
fn grammar_list(name: &str) -> BTreeSet<&'static str> {
    let start = format!("const {name} = [");
    let begin = GRAMMAR
        .find(&start)
        .unwrap_or_else(|| panic!("grammar has no {name} list"))
        + start.len();
    quoted(&GRAMMAR[begin..begin + GRAMMAR[begin..].find("];").unwrap()])
}

fn lower(words: BTreeSet<&str>) -> BTreeSet<&str> {
    words
        .into_iter()
        .filter(|w| w.starts_with(|c: char| c.is_ascii_lowercase()))
        .collect()
}

fn missing<'a>(want: &BTreeSet<&'a str>, have: &BTreeSet<&str>) -> Vec<&'a str> {
    want.iter()
        .filter(|w| !have.contains(*w))
        .copied()
        .collect()
}

#[test]
fn grammar_keywords_match_reserved_words() {
    let reserved = quoted(item(FRONTEND, "pub(crate) fn reserved("));
    let keywords = grammar_list("KEYWORDS");
    let literals = grammar_list("LITERALS");
    let classified: BTreeSet<_> = keywords.union(&literals).copied().collect();
    assert!(
        reserved.len() > 30,
        "reserved words not found: {reserved:?}"
    );
    assert_eq!(
        missing(&reserved, &classified),
        Vec::<&str>::new(),
        "reserved words missing from KEYWORDS or LITERALS"
    );
    assert_eq!(
        missing(&keywords, &reserved),
        Vec::<&str>::new(),
        "KEYWORDS entries the compiler does not reserve"
    );
}

#[test]
fn grammar_literals_match_prelude_variants() {
    let reserved = quoted(item(FRONTEND, "pub(crate) fn reserved("));
    let variants = quoted(item(ENUMS, "pub(super) fn prelude_variant("));
    let literals = grammar_list("LITERALS");
    assert_eq!(missing(&variants, &literals), Vec::<&str>::new());
    let known: BTreeSet<_> = reserved.union(&variants).copied().collect();
    assert_eq!(missing(&literals, &known), Vec::<&str>::new());
}

/// Capitalized built-in names need no list: the grammar shows every
/// capitalized name as a type.
#[test]
fn grammar_classifies_every_lower_case_builtin_name() {
    let types = lower(quoted(item(FRONTEND, "fn named_type(")));
    let builtins: BTreeSet<_> = lower(quoted(item(FRONTEND, "pub(crate) fn builtin(")))
        .into_iter()
        .filter(|w| !w.starts_with("__"))
        .collect();
    let prelude: BTreeSet<_> = types.union(&builtins).copied().collect();
    let grammar_types = grammar_list("TYPES");
    let grammar_builtins = grammar_list("BUILT_INS");
    let grammar: BTreeSet<_> = grammar_types.union(&grammar_builtins).copied().collect();
    assert_eq!(
        missing(&types, &grammar_types),
        Vec::<&str>::new(),
        "primitive types missing from TYPES"
    );
    assert_eq!(
        missing(&prelude, &grammar),
        Vec::<&str>::new(),
        "built-in names missing from TYPES or BUILT_INS"
    );
    assert_eq!(
        missing(&grammar, &prelude),
        Vec::<&str>::new(),
        "TYPES or BUILT_INS entries the compiler does not define"
    );
    assert_eq!(
        grammar_types.intersection(&grammar_builtins).count(),
        0,
        "a name is in both TYPES and BUILT_INS"
    );
}

#[test]
fn book_fences_use_the_plenty_grammars() {
    assert!(
        !GRAMMAR.contains("python"),
        "Plenty fences must not use Python"
    );
    for kind in ["plenty", "plenty-error", "plenty-file", "error", "output"] {
        assert!(
            GRAMMAR.contains(&format!("\"{kind}\"")),
            "`{kind}` fences are not highlighted"
        );
    }
}
