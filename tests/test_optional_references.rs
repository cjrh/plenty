//! `Option[&T]` and `Result[&T, E]` returned from functions (issue #520):
//! book/src/design/language/05-ownership/05-optional-references.md.

mod support;
use support::{assert_stdout, check_error};

const FIND: &str = r#"
class Item:
    name: str
    count: i64
def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing
def find_mut(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing
"#;

fn with_find(body: &str) -> String {
    format!("{FIND}{body}")
}

fn rejects(source: &str, message: &str) {
    let error = check_error(source);
    assert!(
        error.contains(message),
        "{source}\nexpected {message:?}, got {error}"
    );
}

#[test]
fn matched_lookups_borrow_until_the_payload_is_last_used() {
    assert_stdout(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
match find(&items, "ada"):
    case Some(item):
        print(item.count).unwrap()
    case Nothing:
        pass
match find(&items, "bob"):
    case Some(item):
        print(item.count).unwrap()
    case Nothing:
        items.append(Item("bob", 2)).unwrap()
match find_mut(&mut items, "bob"):
    case Some(item):
        item.count += 40
    case Nothing:
        pass
print(items[1].count).unwrap()
"#,
        ),
        "1\n42\n",
    );
}

#[test]
fn a_payload_borrow_conflicts_with_mutation_of_the_origin() {
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
match find(&items, "ada"):
    case Some(item):
        items.append(Item("bob", 2)).unwrap()
        print(item.count).unwrap()
    case Nothing:
        pass
"#,
        ),
        "conflicting borrow: cannot modify or exclusively borrow `items` while it is borrowed",
    );
}

#[test]
fn locals_hold_the_borrow_until_their_last_use() {
    assert_stdout(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
found = find(&items, "ada")
copy = found
print(found.is_some()).unwrap()
print(copy.is_none()).unwrap()
items.append(Item("bob", 2)).unwrap()
print(len(items)).unwrap()
"#,
        ),
        "True\nFalse\n2\n",
    );
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
found = find(&items, "ada")
items.append(Item("bob", 2)).unwrap()
print(found.is_some()).unwrap()
"#,
        ),
        "conflicting borrow: cannot modify or exclusively borrow `items` while it is borrowed",
    );
}

#[test]
fn question_mark_unwrap_and_ok_or_pass_the_borrow_on() {
    assert_stdout(
        &with_find(
            r#"
def count(items: &list[Item], name: &str) -> Option[i64]:
    item = find(items, name)?
    Some(item.count)
def require(items: &list[Item], name: &str) -> Result[&Item, str]:
    find(items, name).ok_or("missing")
items = [Item("ada", 3)].unwrap()
print(count(&items, "ada")).unwrap()
print(count(&items, "bob")).unwrap()
match require(&items, "bob"):
    case Ok(item):
        print(item.count).unwrap()
    case Err(message):
        print(message).unwrap()
found = find(&items, "ada")
item = found.unwrap()
print(item.count).unwrap()
"#,
        ),
        "Option[i64].Some(3)\nOption[i64].Nothing\nmissing\n3\n",
    );
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
item = find(&items, "ada").unwrap()
items.append(Item("bob", 2)).unwrap()
print(item.count).unwrap()
"#,
        ),
        "conflicting borrow",
    );
}

#[test]
fn results_and_nested_sums_carry_projected_references() {
    assert_stdout(
        r#"
def first(xs: &list[i64]) -> Option[&i64]:
    if len(xs) == 0:
        return Nothing
    Some(&xs[0])
def head(xs: &list[str]) -> Result[&str, str]:
    if len(xs) == 0:
        return Err("empty")
    Ok(&xs[0])
def checked(xs: &list[i64]) -> Result[Option[&i64], str]:
    if len(xs) > 5:
        return Err("long")
    Ok(first(xs))
def inner(o: &Option[Option[i64]]) -> Option[&Option[i64]]:
    match o:
        case Some(x):
            Some(x)
        case Nothing:
            Nothing
def slot(o: &mut Option[i64]) -> Option[&mut i64]:
    match o:
        case Some(x):
            Some(x)
        case Nothing:
            Nothing
xs = [7, 8].unwrap()
empty: list[i64] = [].unwrap()
names = ["ada"].unwrap()
print(first(&xs)).unwrap()
print(first(&empty)).unwrap()
print(str.repr(head(&names)).unwrap()).unwrap()
print(str.repr(checked(&xs)).unwrap()).unwrap()
nested: Option[Option[i64]] = Some(Some(5))
print(inner(&nested)).unwrap()
mut c: Option[i64] = Some(1)
match slot(&mut c):
    case Some(x):
        *x = 42
    case Nothing:
        pass
print(c).unwrap()
"#,
        "Option[&i64].Some(7)\nOption[&i64].Nothing\nResult[&str, str].Ok(\"ada\")\n\
Result[Option[&i64], str].Ok(Option[&i64].Some(7))\n\
Option[&Option[i64]].Some(Option[i64].Some(5))\nOption[i64].Some(42)\n",
    );
}

#[test]
fn methods_forward_and_return_held_results() {
    assert_stdout(
        &with_find(
            r#"
class Bag:
    items: list[Item]
    def get(self, name: &str) -> Option[&Item]:
        find(&self.items, name)
def again(items: &list[Item]) -> Option[&Item]:
    found = find(items, "ada")
    found
bag = Bag([Item("ada", 5)].unwrap())
match bag.get("ada"):
    case Some(item):
        print(item.count).unwrap()
    case Nothing:
        pass
print(again(&bag.items).is_some()).unwrap()
"#,
        ),
        "5\nTrue\n",
    );
}

#[test]
fn signatures_need_a_matching_origin_parameter() {
    rejects(
        "class Item:\n    count: i64\ndef bad() -> Option[&Item]:\n    Nothing\n",
        "a function returning Option[&Item] needs a reference parameter to borrow from",
    );
    rejects(
        "class Item:\n    count: i64\ndef bad(items: &list[Item]) -> Option[&mut Item]:\n    Nothing\n",
        "Option[&mut Item] requires the first reference parameter to be `&mut`",
    );
    rejects(
        "class Item:\n    count: i64\ndef bad(items: &list[Item]) -> Result[i64, &Item]:\n    Ok(1)\n",
        "Result errors cannot carry references; copy the part of the input the error reports",
    );
    rejects(
        "def bad(a: &list[str], b: &list[str]) -> Option[&str]:\n    Some(&b[0])\n",
        "returned reference must originate from the first reference parameter",
    );
    rejects(
        "def bad(a: &list[str]) -> Option[&str]:\n    local = \"z\"\n    Some(&local)\n",
        "returned reference must originate from the first reference parameter",
    );
}

#[test]
fn reference_carrying_values_are_never_stored() {
    let stored = |place: &str| {
        format!(
            "an Option or Result carrying a reference cannot be stored in {place}; match it and store an owned copy or an index"
        )
    };
    rejects(
        &with_find("items = [Item(\"ada\", 1)].unwrap()\npair = (find(&items, \"ada\"), 1)\n"),
        &stored("tuples"),
    );
    rejects(
        &with_find("items = [Item(\"ada\", 1)].unwrap()\nxs = [find(&items, \"ada\")].unwrap()\n"),
        &stored("collections"),
    );
    rejects(
        "class Holder:\n    o: Option[&str]\n",
        &stored("class fields"),
    );
    rejects(
        "enum Found:\n    Hit(Option[&str])\n",
        &stored("enum payloads"),
    );
    rejects(
        &with_find(
            "def keep[T](x: T) -> T:\n    x\nitems = [Item(\"ada\", 1)].unwrap()\no = keep(find(&items, \"ada\"))\n",
        ),
        "reference type arguments are not supported",
    );
}

#[test]
fn later_phases_stay_rejected() {
    // Phase 2 of book/src/proposals/returned-reference-enums.md: delete
    // these rejections when `mut` holders and parameters land (B11).
    rejects(
        &with_find("items = [Item(\"ada\", 1)].unwrap()\nmut o = find(&items, \"ada\")\n"),
        "`mut o` cannot hold Option[&Item]; bind each result to its own local",
    );
    rejects(
        &with_find("def take(o: Option[&Item]) -> ():\n    pass\n"),
        "parameter `o` cannot be Option[&Item]",
    );
    rejects(
        "n: Option[&str] = Nothing\n",
        "a binding of type Option[&str] must hold a borrow, and this value holds none",
    );
}

#[test]
fn rejected_uses_name_the_alternative() {
    rejects(
        &with_find(
            r#"
items = [Item("ada", 1)].unwrap()
o = find(&items, "ada")
match &o:
    case Some(item):
        print(item.count).unwrap()
    case Nothing:
        pass
"#,
        ),
        "`o` already holds a reference; match `o` directly",
    );
    rejects(
        &with_find(
            r#"
items = [Item("ada", 1)].unwrap()
other = Item("bob", 2)
item = find(&items, "ada").unwrap_or(&other)
"#,
        ),
        "`unwrap_or` cannot choose between two borrows; use `match`",
    );
    rejects(
        &with_find(
            r#"
items = [Item("ada", 1)].unwrap()
o = find(&items, "ada")
f = def [o]() -> bool:
    o.is_some()
"#,
        ),
        "`o` holds a borrow and cannot be captured; match it first and pass the reference",
    );
    rejects(
        &with_find(
            r#"
def counts(items: list[Item]) -> Generator[i64]:
    o = find(&items, "ada")
    yield 1
    print(o.is_some()).unwrap()
"#,
        ),
        "a borrow of `items` cannot remain live across yield",
    );
    rejects(
        &with_find(
            r#"
def names() -> Result[list[Item], AllocError]:
    [Item("ada", 1)]
o = find(&names().unwrap(), "ada")
print(o.is_some()).unwrap()
"#,
        ),
        "a reference to a temporary cannot be stored beyond its full expression",
    );
}
