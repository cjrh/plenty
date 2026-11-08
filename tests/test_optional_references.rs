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
def main() -> Result[(), OverflowError[i64]]:
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
    Ok(())
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

/// Issue #868: a parameter holding `Option[&T]` or `Result[&T, E]` takes
/// the caller's borrow and can be the origin of a returned one.
#[test]
fn carried_parameters_lend_the_callers_borrow() {
    assert_stdout(
        &with_find(
            r#"
def show(value: Option[&Item]) -> Result[(), IoError]:
    match value:
        case Some(item):
            print(item.name)?
        case Nothing:
            print("missing")?
    Ok(())

def rename(value: Option[&mut Item]) -> ():
    match value:
        case Some(item):
            item.count = 7
        case Nothing:
            pass

def relay(value: Option[&Item]) -> Option[&Item]:
    value

def relay_mut(value: Option[&mut Item]) -> Option[&mut Item]:
    value

def name_of(value: Option[&Item]) -> Option[&str]:
    match value:
        case Some(item):
            Some(&item.name)
        case Nothing:
            Nothing

def checked(value: Result[&Item, str]) -> Result[&Item, str]:
    item = value?
    Ok(item)

def kept[T](value: Option[&T]) -> Option[&T]:
    value

class Shelf:
    items: list[Item]

    def first(self: &Shelf, skip: Option[&str]) -> Option[&Item]:
        for item in &self.items:
            match skip:
                case Some(name):
                    if item.name == name:
                        continue
                case Nothing:
                    pass
            return Some(item)
        Nothing

def each[F: Callable[[Option[&Item]], Result[(), IoError]]](f: &F, item: &Item) -> Result[(), IoError]:
    f(Some(item))?
    f(Nothing)

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    found = find(&items, "ada")
    show(found)?
    show(found)?
    show(Nothing)?
    show(relay(find(&items, "zed")))?
    match name_of(kept(found)):
        case Some(name):
            print(name)?
        case Nothing:
            pass
    match checked(Ok(&items[1])):
        case Ok(item):
            print(item.name)?
        case Err(message):
            print(message)?
    rename(find_mut(&mut items, "bob"))
    held = relay_mut(find_mut(&mut items, "ada"))
    rename(held)
    rename(held)
    print(items[0].count)?
    print(items[1].count)?
    each(&show, &items[0])?
    shelf = Shelf(items)
    skip = "ada"
    match shelf.first(Some(&skip)):
        case Some(item):
            print(item.name)?
        case Nothing:
            pass
    Ok(())
"#,
        ),
        "ada\nada\nmissing\nmissing\nada\nbob\n7\n7\nada\nmissing\nbob\n",
    );
}

/// The issue's examples, a nested `Result[Option[&T], E]` parameter, and a
/// `mut` holder passed before and after it adopts a place.
#[test]
fn carried_parameters_take_nested_results_and_holders() {
    assert_stdout(
        r#"
class Item:
    name: str

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def show(o: Option[&str]) -> ():
    drop(o)

def show_name(selected: Option[&Item]) -> Result[(), IoError]:
    match selected:
        case Some(item):
            print(item.name)?
        case Nothing:
            print("none")?
    Ok(())

def lookup(items: &list[Item], name: &str) -> Result[Option[&Item], str]:
    if name == "":
        return Err("empty")
    Ok(find(items, name))

def report(found: Result[Option[&Item], str]) -> Result[Option[&Item], str]:
    inner = found?
    show_name(inner).unwrap()
    Ok(inner)

def main() -> Result[(), AllocError | IoError]:
    items = [Item("ada"), Item("bob")]?
    show(Nothing)
    mut selected: Option[&Item] = Nothing
    show_name(selected)?
    selected = find(&items, "bob")
    show_name(selected)?
    first = "ada"
    print(report(lookup(&items, &first)).is_ok())?
    empty = ""
    print(report(lookup(&items, &empty)).is_err())?
    Ok(())
"#,
        "none\nbob\nada\nTrue\nTrue\n",
    );
}

#[test]
fn carried_parameter_results_keep_the_argument_borrowed() {
    rejects(
        &with_find(
            r#"
def relay(value: Option[&Item]) -> Option[&Item]:
    value
mut items = [Item("ada", 1)].unwrap()
kept = relay(find(&items, "ada"))
items.clear()
print(kept.is_some()).unwrap()
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    // Passing an exclusive holder reborrows it, so the holder cannot be
    // used while a result that keeps its borrow is live.
    rejects(
        &with_find(
            r#"
def relay_mut(value: Option[&mut Item]) -> Option[&mut Item]:
    value
mut items = [Item("ada", 1)].unwrap()
held = find_mut(&mut items, "ada")
kept = relay_mut(held)
again = relay_mut(held)
print(kept.is_some()).unwrap()
"#,
        ),
        "cannot modify or exclusively borrow `items` (through `held`) while it is exclusively borrowed",
    );
    rejects(
        "class Item:\n    count: i64\ndef bad(value: Option[&Item], other: &Item) -> &Item:\n    other\n",
        "returned reference must originate from the first reference parameter, `value`; this one borrows `other`",
    );
    rejects(
        "class Item:\n    count: i64\ndef bad(other: &Item, value: Option[&Item]) -> Option[&Item]:\n    value\n",
        "returned reference must originate from the first reference parameter, `other`; this one borrows `value`",
    );
    rejects(
        "class Item:\n    count: i64\ndef bad(value: Option[&Item]) -> Option[&mut Item]:\n    Nothing\n",
        "Option[&mut Item] requires the first reference parameter to be `&mut`",
    );
    // A later phase of book/src/proposals/returned-reference-enums.md (B11).
    rejects(
        "def take(o: &Option[&str]) -> ():\n    pass\n",
        "parameter `o` cannot be &Option[&str]: pass the Option[&str] itself",
    );
    rejects(
        "def take(o: Option[&str]) -> Generator[i64]:\n    yield 1\n",
        "generators cannot capture references",
    );
}

/// Issue #868: a `mut` holder that starts empty borrows the place of the
/// first value assigned to it, from that assignment on.
#[test]
fn mut_holders_adopt_their_first_place() {
    assert_stdout(
        &with_find(
            r#"
def longest(items: &list[Item]) -> Option[&Item]:
    mut best: Option[&Item] = Nothing
    mut size = 0
    for item in items:
        if len(item.name) > size:
            size = len(item.name)
            best = Some(item)
        if size > 100:
            best = Nothing
    best

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("a", 1), Item("bbb", 2), Item("cc", 3)]?
    mut last: Option[&mut Item] = Nothing
    items.append(Item("dd", 4))?
    for name in ["a", "cc"]?:
        last = find_mut(&mut items, name)
    match last:
        case Some(item):
            item.count = 30
        case Nothing:
            pass
    items.append(Item("e", 5))?
    match longest(&items):
        case Some(item):
            print(item.name)?
        case Nothing:
            print("none")?
    print(items[2].count)?
    mut first: Result[&Item, str] = Err("empty")
    first = Ok(&items[0])
    first = Err("reset")
    match first:
        case Ok(item):
            print(item.name)?
        case Err(message):
            print(message)?
    Ok(())
"#,
        ),
        "bbb\n30\nreset\n",
    );
}

#[test]
fn mut_holders_keep_their_borrows_until_the_last_use() {
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
mut best: Option[&Item] = Nothing
for item in &items:
    best = Some(item)
items.clear()
print(best.is_some()).unwrap()
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    // An earlier `return` in a loop can return a value assigned later.
    rejects(
        &with_find(
            r#"
def first(items: &list[Item], other: &list[Item]) -> Option[&Item]:
    mut best: Option[&Item] = Nothing
    for item in other:
        if best.is_some():
            return best
        best = Some(item)
    Nothing
"#,
        ),
        "returned reference must originate from the first reference parameter, `items`; this one borrows `other`",
    );
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1)].unwrap()
mut cur: Option[&mut Item] = Nothing
cur = find_mut(&mut items, "ada")
print(items[0].count).unwrap()
print(cur.is_some()).unwrap()
"#,
        ),
        "cannot read or borrow `items[0].count` while `items` is exclusively borrowed",
    );
    rejects(
        &with_find(
            r#"
mut items = [Item("ada", 1), Item("bob", 2)].unwrap()
mut a: Option[&mut Item] = Nothing
mut b: Option[&mut Item] = Nothing
a = find_mut(&mut items, "ada")
b = find_mut(&mut items, "bob")
print(a.is_some()).unwrap()
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
}

/// Every use of a holder that started empty keeps every value assigned to
/// it borrowed, whatever their order in the body, on the paths where some
/// assignment ran.
#[test]
fn mut_holders_keep_every_assigned_borrow() {
    // A use earlier in a loop body than the assignment sees the previous iteration's value.
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    for k in [1, 2, 3]?:
        items.clear()
        items.append(Item("zzz", k))?
        print(best)?
        best = find(&items, "zzz")
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def put(o: Option[&mut Item], v: i64) -> ():
    match o:
        case Some(i):
            i.count = v
        case Nothing:
            pass
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut cur: Option[&mut Item] = Nothing
    for k in [1, 2, 3]?:
        other = find_mut(&mut items, "ada")
        put(cur, 10)
        put(other, 20)
        cur = find_mut(&mut items, "ada")
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
    // An assignment on a path that skips the first one lowered still holds its borrow.
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    if len(items) > 5:
        best = find(&items, "ada")
        print(best)?
        return Ok(())
    best = find(&items, "ada")
    items.clear()
    print(best)?
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    if len(items) > 5:
        best = find(&items, "ada")
        print(best)?
        return Ok(())
    else:
        best = find(&items, "ada")
    items.clear()
    print(best)?
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    for k in [1, 2, 3]?:
        if k == 1:
            best = find(&items, "ada")
            print(best)?
            return Ok(())
        best = find(&items, "bob")
        items.append(Item("x", k))?
        print(best)?
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def put(o: Option[&mut Item], v: i64) -> ():
    match o:
        case Some(i):
            i.count = v
        case Nothing:
            pass
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut cur: Option[&mut Item] = Nothing
    if len(items) > 5:
        cur = find_mut(&mut items, "ada")
        put(cur, 1)
        return Ok(())
    cur = find_mut(&mut items, "ada")
    items.clear()
    put(cur, 10)
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
    // A copy, or a payload taken before a reassignment, keeps the earlier value.
    rejects(
        &with_find(
            r#"
def put(o: Option[&mut Item], v: i64) -> ():
    match o:
        case Some(i):
            i.count = v
        case Nothing:
            pass
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    mut cur: Option[&mut Item] = Nothing
    cur = find_mut(&mut items, "ada")
    keep = cur
    cur = find_mut(&mut items, "bob")
    put(keep, 1)
    put(cur, 2)
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
    rejects(
        &with_find(
            r#"
def put(o: Option[&mut Item], v: i64) -> ():
    match o:
        case Some(i):
            i.count = v
        case Nothing:
            pass
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    mut cur: Option[&mut Item] = Nothing
    cur = find_mut(&mut items, "ada")
    match cur:
        case Some(i):
            cur = find_mut(&mut items, "bob")
            i.count = 9
        case Nothing:
            pass
    put(cur, 1)
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
}

/// A holder assigned another started-empty holder keeps every value assigned
/// to that one borrowed, and returns them all.
#[test]
fn mut_holders_keep_values_assigned_from_other_holders() {
    const PUT: &str = r#"
def put(o: Option[&mut Item], v: i64) -> ():
    match o:
        case Some(i):
            i.count = v
        case Nothing:
            pass
"#;
    assert_stdout(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    mut best: Option[&Item] = Nothing
    mut keep: Option[&Item] = Nothing
    keep = find(&items, "bob")
    for name in ["ada", "bob"]?:
        best = find(&items, name)
        if name == "ada":
            keep = best
    print(keep)?
    items.clear()
    Ok(())
"#,
        ),
        "Option[&Item].Some(Item(name=\"ada\", count=1))\n",
    );
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    mut keep: Option[&Item] = Nothing
    best = find(&items, "ada")
    keep = best
    items.clear()
    print(keep)?
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut best: Option[&Item] = Nothing
    mut keep: Option[&Item] = Nothing
    for k in [0, 1]?:
        best = find(&items, "ada")
        if k == 0:
            keep = best
    items.clear()
    print(keep)?
    Ok(())
"#,
        ),
        "cannot modify or exclusively borrow `items` while it is borrowed",
    );
    rejects(
        &with_find(&format!(
            r#"{PUT}
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    mut cur: Option[&mut Item] = Nothing
    mut keep: Option[&mut Item] = Nothing
    cur = find_mut(&mut items, "ada")
    keep = cur
    other = find_mut(&mut items, "ada")
    put(keep, 1)
    put(other, 2)
    Ok(())
"#
        )),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
    // The return check covers values assigned to the source holder before
    // and after the copy.
    for body in [
        "    a = find(&local, \"z\")\n    b = a",
        "    b = find(items, \"z\")\n    for k in [0, 1]?:\n        b = a\n        a = find(&local, \"z\")",
    ] {
        rejects(
            &with_find(&format!(
                "
def pick(items: &list[Item], local: list[Item]) -> Result[Option[&Item], AllocError]:
    mut a: Option[&Item] = Nothing
    mut b: Option[&Item] = Nothing
{body}
    Ok(b)
def main() -> ():
    pass
"
            )),
            "returned reference must originate from the first reference parameter, `items`",
        );
    }
    // A conflict on a value a holder keeps names the holder.
    rejects(
        &with_find(&format!(
            r#"{PUT}
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    mut cur: Option[&mut Item] = Nothing
    cur = find_mut(&mut items, "ada")
    put(cur, 5)
    cur = find_mut(&mut items, "bob")
    put(cur, 6)
    Ok(())
"#
        )),
        "`cur` started empty, so it keeps every value assigned to it borrowed; use a separate binding per value",
    );
}

/// Issue #867: an empty or error-only holder has no referent and borrows no
/// place, so its origin stays free to change while the holder is live.
#[test]
fn empty_holders_borrow_nothing() {
    assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    o: Option[&str] = Nothing
    r: Result[&str, i64] = Err(1)
    copy = o
    print(o)?
    print(str.repr(r)?)?
    print(copy.is_none())?
    print(r.is_err())?
    match o:
        case Some(text):
            print(text)?
        case Nothing:
            print("none")?
    match r:
        case Ok(text):
            print(text)?
        case Err(code):
            print(code)?
    drop(o)
    Ok(())
"#,
        "Option[&str].Nothing\nResult[&str, i64].Err(1)\nTrue\nTrue\nnone\n1\n",
    );
    assert_stdout(
        &with_find(
            r#"
def pick(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    missing: Option[&mut Item] = Nothing
    unused: Option[&mut Item] = Nothing
    items[0].count = 5
    match unused:
        case Some(item):
            item.count = 7
        case Nothing:
            pass
    for item in items:
        if item.name == name:
            return Some(item)
    missing

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1)]?
    match pick(&mut items, "bob"):
        case Some(item):
            item.count = 9
        case Nothing:
            print("missing")?
    print(items[0].count)?
    Ok(())
"#,
        ),
        "missing\n5\n",
    );
}

#[test]
fn nested_empty_holders_return_without_an_origin() {
    assert_stdout(
        r#"
def first(words: &list[str]) -> Option[&str]:
    nested: Option[Option[&str]] = Some(Nothing)
    match nested:
        case Some(found):
            if len(words) == 0:
                return found
        case Nothing:
            pass
    empty: Option[&str] = Nothing
    wrapped: Result[Option[&str], i64] = Ok(empty)
    match wrapped:
        case Ok(found):
            if len(words) == 1:
                return found
        case Err(_):
            pass
    Some(&words[0])

def main() -> Result[(), AllocError | IoError]:
    none: list[str] = []?
    print(first(&none))?
    one = ["a"]?
    print(first(&one))?
    two = ["a", "b"]?
    print(first(&two))?
    Ok(())
"#,
        "Option[&str].Nothing\nOption[&str].Nothing\nOption[&str].Some(\"a\")\n",
    );
}

#[test]
fn empty_holders_keep_real_borrows_and_inference_checked() {
    rejects(
        r#"
def escape(words: &list[str]) -> Option[&str]:
    local = "x"
    empty: Option[&str] = Nothing
    match empty:
        case Some(_):
            return Some(&local)
        case Nothing:
            pass
    Nothing
"#,
        "returned reference must originate from the first reference parameter",
    );
    rejects(
        r#"
def escape(words: &list[str]) -> Option[&str]:
    local = "x"
    o = Some(&local)
    o
"#,
        "returned reference must originate from the first reference parameter",
    );
    // The empty holder's loan precedes the real borrow in these returned
    // values, so the real borrow must still be the origin that is checked.
    rejects(
        r#"
def escape(words: &list[str]) -> Option[&str]:
    local = "x"
    s = &local
    o: Option[&str] = Nothing
    drop(o)
    Some(s)
"#,
        "returned reference must originate from the first reference parameter",
    );
    rejects(
        r#"
def h(text: &str, flag: bool) -> &str:
    text
def escape(words: &list[str]) -> &str:
    local = "x"
    o: Option[&str] = Nothing
    h(&local, o.is_none())
"#,
        "returned reference must originate from the first reference parameter",
    );
    // A conflict on the hidden referent names the empty holder.
    rejects(
        r#"
o: Option[&mut i64] = Nothing
q = o
match o:
    case Some(v):
        *v = 2
    case Nothing:
        pass
match q:
    case Some(w):
        *w = 3
    case Nothing:
        pass
"#,
        "cannot modify or exclusively borrow `o` (through `o`)",
    );
    rejects(
        "o = Nothing\ndrop(o)\n",
        "cannot infer `Nothing`; provide an Option annotation",
    );
    rejects("o: Option[&str] = Some(1)\n", "expected &str, got i64");
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

#[test]
fn branch_values_carry_each_arms_borrow() {
    assert_stdout(
        &with_find(
            r#"
def pick(items: &list[Item], first: bool) -> Option[&Item]:
    return if first:
        find(items, "ada")
    else:
        find(items, "bob")
def pick_tail(items: &list[Item], first: bool) -> Option[&Item]:
    chosen = match first:
        case True:
            find(items, "ada")
        case False:
            Nothing
    chosen
def first(items: &list[Item]) -> Result[&Item, str]:
    if len(items) > 0:
        return Ok(&items[0])
    Err("empty")
def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    others = [Item("cy", 3)]?
    many = len(items) > 1
    selected = if many:
        find(&items, "bob")
    else:
        find(&others, "cy")
    print(selected)?
    nested = match selected:
        case Some(item):
            if item.count > 1:
                find(&items, "ada")
            else:
                Nothing
        case Nothing:
            Nothing
    print(nested)?
    result = if many:
        first(&items)
    else:
        Err("skipped")
    match result:
        case Ok(item):
            print(item.name)?
        case Err(e):
            print(e)?
    print(pick(&items, False))?
    print(pick_tail(&items, True))?
    changed = if many:
        find_mut(&mut items, "ada")
    else:
        Nothing
    match changed:
        case Some(item):
            item.count = 7
        case Nothing:
            pass
    items.clear()
    print(len(items))?
    Ok(())
"#,
        ),
        "Option[&Item].Some(Item(name=\"bob\", count=2))\n\
         Option[&Item].Some(Item(name=\"ada\", count=1))\n\
         ada\n\
         Option[&Item].Some(Item(name=\"bob\", count=2))\n\
         Option[&Item].Some(Item(name=\"ada\", count=1))\n\
         0\n",
    );
}

#[test]
fn branch_values_keep_every_arm_borrowed() {
    const NOTE: &str =
        "holds this arm's value: an `if` or `match` value keeps the value of every arm borrowed";
    for (body, message) in [
        (
            r#"
mut a = [Item("ada", 1)].unwrap()
b = [Item("bob", 2)].unwrap()
x = if len(b) > 0:
    find(&b, "bob")
else:
    find(&a, "ada")
a.clear()
print(x).unwrap()
"#,
            "cannot modify or exclusively borrow `a` while it is borrowed",
        ),
        (
            r#"
mut items = [Item("ada", 1)].unwrap()
x = if len(items) > 0:
    y = if len(items) > 1:
        find(&items, "ada")
    else:
        Nothing
    y
else:
    Nothing
items.clear()
print(x).unwrap()
"#,
            "cannot modify or exclusively borrow `items` while it is borrowed",
        ),
        (
            r#"
mut items = [Item("ada", 1)].unwrap()
a = match len(items):
    case 0:
        find_mut(&mut items, "ada")
    case _:
        find_mut(&mut items, "bob")
b = find_mut(&mut items, "ada")
print(b.is_some()).unwrap()
print(a.is_some()).unwrap()
"#,
            "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
        ),
        (
            r#"
mut items = [Item("ada", 1)].unwrap()
mut keep: Option[&Item] = Nothing
for k in [0, 1].unwrap():
    x = if k == 0:
        find(&items, "ada")
    else:
        keep
    keep = x
    items.clear()
print(keep).unwrap()
"#,
            "cannot modify or exclusively borrow `items` while it is borrowed",
        ),
    ] {
        let source = with_find(body);
        rejects(&source, message);
        rejects(&source, NOTE);
    }
}

#[test]
fn branch_values_cannot_outlive_their_arms() {
    rejects(
        &with_find(
            r#"
items = [Item("ada", 1)].unwrap()
x = if len(items) > 0:
    local = [Item("zzz", 1)].unwrap()
    find(&local, "zzz")
else:
    Nothing
print(x).unwrap()
"#,
        ),
        "cannot drop `local` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def make() -> Result[list[Item], AllocError]:
    [Item("ada", 1)]
x = if True:
    find(&make().unwrap(), "ada")
else:
    Nothing
print(x).unwrap()
"#,
        ),
        "cannot drop a temporary value while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def pick(items: &list[Item], other: &list[Item], first: bool) -> Option[&Item]:
    return if first:
        find(items, "ada")
    else:
        find(other, "bob")
"#,
        ),
        "returned reference must originate from the first reference parameter, `items`; this one borrows `other`",
    );
    rejects(
        &with_find(
            r#"
items = [Item("ada", 1)].unwrap()
x: Option[&Item] = checked:
    find(&items, "ada")
print(x).unwrap()
"#,
        ),
        "a `checked:` block cannot produce a reference",
    );
}

#[test]
fn branch_values_borrow_only_through_the_arm_that_ran() {
    // Review of local PR #1106: two exclusive arms of one owner, with
    // arguments that leave no temporary after the loan, and loops whose
    // value is used only within its iteration.
    assert_stdout(
        &with_find(
            r#"
def head_mut(xs: &mut list[i64], k: i64) -> Option[&mut i64]:
    for x in xs:
        return Some(x)
    Nothing
def touch(o: Option[&mut i64]) -> ():
    pass
def show(o: Option[&Item]) -> ():
    pass
def bump(o: Option[&mut Item]) -> ():
    match o:
        case Some(item):
            item.count = 9
        case Nothing:
            pass
def main() -> Result[(), AllocError | IoError]:
    mut numbers = [1, 2]?
    many = len(numbers) > 1
    z = if many:
        head_mut(&mut numbers, 1)
    else:
        head_mut(&mut numbers, 2)
    touch(z)
    always = if True:
        head_mut(&mut numbers, 1)
    else:
        head_mut(&mut numbers, 2)
    touch(always)
    who = "ada"
    mut a = [Item("ada", 1)]?
    mut b = [Item("bob", 1)]?
    w = match many:
        case True:
            find_mut(&mut a, who)
        case False:
            find_mut(&mut a, who)
    bump(w)
    print(a[0].count)?
    for k in [0, 1, 2]?:
        x = if k == 1:
            find(&a, "ada")
        else:
            Nothing
        show(x)
        a.append(Item("cy", 1))?
        y = if k == 1:
            find(&a, "ada")
        else:
            find(&b, "bob")
        show(y)
        a.append(Item("dee", 1))?
        b.append(Item("eve", 1))?
        m = if k == 1:
            find_mut(&mut a, "ada")
        else:
            find_mut(&mut a, "bob")
        bump(m)
        show(find(&a, "ada"))
    print(len(a))?
    Ok(())
"#,
        ),
        "9\n7\n",
    );
}

#[test]
fn branch_values_copied_out_of_an_iteration_stay_borrowed() {
    for body in [
        r#"
mut b = [Item("bob", 1)].unwrap()
c = [Item("cy", 1)].unwrap()
mut keep: Option[&Item] = Nothing
for k in [0, 1, 2].unwrap():
    b.clear()
    x = if k == 0:
        find(&b, "bob")
    else:
        find(&c, "cy")
    if k == 0:
        keep = x
    print(keep).unwrap()
"#,
        r#"
mut b = [Item("bob", 1)].unwrap()
c = [Item("cy", 1)].unwrap()
mut x: Option[&Item] = Nothing
for k in [0, 1, 2].unwrap():
    b.clear()
    print(x).unwrap()
    x = if k == 0:
        find(&b, "bob")
    else:
        find(&c, "cy")
"#,
        r#"
mut b = [Item("bob", 1)].unwrap()
c = [Item("cy", 1)].unwrap()
mut keep: Option[&Item] = Nothing
for k in [0, 1, 2].unwrap():
    b.clear()
    print(keep).unwrap()
    x = if k == 0:
        find(&b, "bob")
    else:
        find(&c, "cy")
    keep = match x:
        case Some(i):
            Some(i)
        case Nothing:
            Nothing
"#,
    ] {
        rejects(
            &with_find(body),
            "cannot modify or exclusively borrow `b` while it is borrowed",
        );
    }
}

/// A value copied out of one iteration stays borrowed while the next
/// iteration evaluates the same `if`, `unwrap_or` or empty holder again
/// (issue #868): their hidden roots are initialized once per call.
#[test]
fn branch_values_reevaluate_while_an_earlier_copy_is_live() {
    assert_stdout(
        &with_find(
            r#"
def main() -> Result[(), AllocError | IoError]:
    a = [Item("ada", 1)]?
    b = [Item("bob", 1)]?
    other = Item("zed", 0)
    mut keep: Option[&Item] = Nothing
    mut kept: Option[&Item] = Nothing
    mut fallback: Option[&Item] = Nothing
    for k in [0, 1]?:
        x = if k == 0:
            find(&a, "ada")
        else:
            find(&b, "bob")
        mut h: Option[&Item] = Nothing
        if k == 0:
            h = find(&b, "bob")
        r = find(&a, "q").unwrap_or(&other)
        if k == 0:
            keep = x
            kept = h
            fallback = Some(r)
        print(x)?
    print(keep)?
    print(kept)?
    print(fallback.unwrap().name)?
    Ok(())
"#,
        ),
        "Option[&Item].Some(Item(name=\"ada\", count=1))\nOption[&Item].Some(Item(name=\"bob\", count=1))\nOption[&Item].Some(Item(name=\"ada\", count=1))\nOption[&Item].Some(Item(name=\"bob\", count=1))\nzed\n",
    );
    // The copy still keeps every owner the expression could borrow.
    rejects(
        &with_find(
            r#"
mut a = [Item("ada", 1)].unwrap()
b = [Item("bob", 1)].unwrap()
mut keep: Option[&Item] = Nothing
for k in [0, 1].unwrap():
    x = if k == 0:
        find(&a, "ada")
    else:
        find(&b, "bob")
    if k == 0:
        keep = x
a.clear()
print(keep).unwrap()
"#,
        ),
        "cannot modify or exclusively borrow `a` while it is borrowed",
    );
    // An exclusive copy blocks the next evaluation; the note names the owners.
    rejects(
        &with_find(
            r#"
mut a = [Item("ada", 1)].unwrap()
mut b = [Item("bob", 1)].unwrap()
mut keep: Option[&mut Item] = Nothing
for k in [0, 1].unwrap():
    x = if k == 0:
        find_mut(&mut a, "ada")
    else:
        find_mut(&mut b, "bob")
    if k == 0:
        keep = x
print(keep).unwrap()
"#,
        ),
        "`keep` still holds an earlier value of `x`, which borrows `a` or `b`; `x` cannot borrow them again while `keep` is live",
    );
}

/// A holder that started empty may be assigned borrows of different owners
/// (issue #868); each use keeps all of them borrowed.
#[test]
fn empty_holders_choose_between_owners() {
    let program = r#"
class Item:
    name: str
    count: i64

def main() -> Result[(), AllocError | IoError]:
    mut a = [Item("ada", 1)]?
    mut b = [Item("bob", 2)]?
    mut pick: Option[&Item] = Nothing
    if len(a) > 5:
        pick = Some(&a[0])
    else:
        pick = Some(&b[0])
    match pick:
        case Some(item):
            print(item.name)?
        case Nothing:
            pass
    pick = Some(&a[0])
    match pick:
        case Some(item):
            print(item.name)?
        case Nothing:
            pass
    a.clear()
    b.clear()
    Ok(())
"#;
    assert_stdout(program, "bob\nada\n");
    rejects(
        &with_find(
            r#"
mut a = [Item("ada", 1)].unwrap()
b = [Item("bob", 1)].unwrap()
mut pick: Option[&Item] = Nothing
pick = Some(&a[0])
pick = Some(&b[0])
a.clear()
print(pick.is_some()).unwrap()
"#,
        ),
        "`pick` started empty, so it keeps every value assigned to it borrowed",
    );
    rejects(
        &with_find(
            r#"
a = [Item("ada", 1)].unwrap()
b = [Item("bob", 1)].unwrap()
mut pick = find(&a, "ada")
pick = find(&b, "bob")
"#,
        ),
        "can be reassigned only to a reference into the owner it first borrowed; to choose between owners, start it empty, as in `mut pick: Option[&Item] = Nothing`",
    );
    rejects(
        &with_find(
            r#"
def get(items: &list[Item]) -> Result[&Item, str]:
    for item in items:
        return Ok(item)
    Err("none")
a = [Item("ada", 1)].unwrap()
b = [Item("bob", 1)].unwrap()
mut pick = get(&a)
pick = get(&b)
"#,
        ),
        "to choose between owners, start it empty, as in `mut pick: Result[&Item, str] = Err(error)`",
    );
    // A plain reference cannot start empty, so it gets no hint.
    let error = check_error(&with_find(
        "a = [Item(\"ada\", 1)].unwrap()\nb = [Item(\"bob\", 1)].unwrap()\nmut pick = &a[0]\npick = &b[0]\nprint(pick.name).unwrap()\n",
    ));
    assert!(
        error.ends_with("can be reassigned only to a reference into the owner it first borrowed"),
        "{error}"
    );
    let program = r#"
class Item:
    name: str

def get(items: &list[Item]) -> Result[&Item, str]:
    for item in items:
        return Ok(item)
    Err("none")

def main() -> Result[(), AllocError | IoError]:
    a = [Item("ada")]?
    b = [Item("bob")]?
    mut pick: Result[&Item, str] = Err("none")
    pick = get(&a)
    pick = get(&b)
    match pick:
        case Ok(item):
            print(item.name)?
        case Err(e):
            print(e)?
    Ok(())
"#;
    assert_stdout(program, "bob\n");
}

/// `unwrap_or` with a borrowed default keeps the payload's and the
/// default's owners borrowed (issue #868).
#[test]
fn unwrap_or_takes_a_borrowed_default() {
    let program = r#"
class Item:
    name: str
    count: i64

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def first(v: &mut list[i64]) -> Result[&mut i64, str]:
    for x in v:
        return Ok(x)
    Err("empty")

def pick(items: &list[Item], name: &str) -> &Item:
    find(items, name).unwrap_or(&items[0])

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    other = Item("zed", 0)
    print(find(&items, "bob").unwrap_or(&other).name)?
    print(find(&items, "x").unwrap_or(&other).name)?
    print(pick(&items, "x").name)?
    items.clear()
    mut xs: list[i64] = []?
    mut spare = 0
    slot = first(&mut xs).unwrap_or(&mut spare)
    *slot = 5
    print(spare)?
    Ok(())
"#;
    assert_stdout(program, "bob\nzed\nada\n5\n");
    for (body, message) in [
        (
            "mut items = [Item(\"ada\", 1)].unwrap()\nother = Item(\"bob\", 2)\nitem = find(&items, \"x\").unwrap_or(&other)\nitems.clear()\nprint(item.name).unwrap()\n",
            "`item` holds an `unwrap_or` result, which keeps both the payload and the default borrowed",
        ),
        (
            "items = [Item(\"ada\", 1)].unwrap()\nmut other = Item(\"bob\", 2)\nitem = find(&items, \"x\").unwrap_or(&other)\nother.count = 3\nprint(item.name).unwrap()\n",
            "cannot modify or exclusively borrow `other.count` while `other` is borrowed",
        ),
        (
            "def pick(items: &list[Item]) -> &Item:\n    other = Item(\"bob\", 2)\n    find(items, \"x\").unwrap_or(&other)\n",
            "returned reference must originate from the first reference parameter, `items`; the default of `unwrap_or` must also borrow from `items`",
        ),
    ] {
        rejects(&with_find(body), message);
    }
}

/// A pattern below the carried reference matches its referent and binds
/// references into it (issue #868).
#[test]
fn nested_patterns_match_through_the_carried_reference() {
    let program = r#"
class Item:
    name: str
    count: i64

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def first(v: &mut list[Option[i64]]) -> Option[&mut Option[i64]]:
    for x in v:
        return Some(x)
    Nothing

def peek(v: &list[Option[i64]]) -> Result[&Option[i64], str]:
    for x in v:
        return Ok(x)
    Err("empty")

def describe(v: &list[Option[i64]]) -> Result[(), IoError]:
    match peek(v):
        case Ok(Some(1) | Nothing):
            print("one or none")?
        case Ok(Some(n)):
            print("some", *n)?
        case Err(e):
            print(e)?
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    items = [Item("ada", 3), Item("bob", 2)]?
    for name in ["ada", "bob", "x"]?:
        match find(&items, name):
            case Some(Item(name=n, count=3)):
                print("three", n)?
            case Some(Item(name=n, count=c)):
                print(n, *c)?
            case Nothing:
                print("missing")?
    mut xs = [Some(1), Nothing]?
    match first(&mut xs):
        case Some(Some(v)):
            *v += 10
        case Some(Nothing) | Nothing:
            pass
    print(xs)?
    describe(&xs)?
    describe(&[Nothing]?)?
    describe(&[]?)?
    Ok(())
"#;
    assert_stdout(
        program,
        "three ada\nbob 2\nmissing\n[Option[i64].Some(11), Option[i64].Nothing]\nsome 11\none or none\nempty\n",
    );
    for (body, message) in [
        (
            "mut items = [Item(\"ada\", 1)].unwrap()\nmatch find(&items, \"ada\"):\n    case Some(Item(name=n, count=_)):\n        items.clear()\n        print(n).unwrap()\n    case Nothing:\n        pass\n",
            "cannot modify or exclusively borrow `items` while it is borrowed",
        ),
        (
            "items = [Item(\"ada\", 1)].unwrap()\nmatch find(&items, \"ada\"):\n    case Some(Item(name=_, count=c)):\n        *c = 1\n    case Nothing:\n        pass\n",
            "assignment requires an exclusive reference",
        ),
    ] {
        rejects(&with_find(body), message);
    }
}

/// Several exclusive bindings below one payload reference are disjoint
/// leaves sharing its loan; a named subject and alternatives work too.
#[test]
fn nested_bindings_below_an_exclusive_reference_coexist() {
    let program = r#"
class Item:
    name: str
    count: i64

def find_mut(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def first(values: &mut list[Option[i64]]) -> Option[&mut Option[i64]]:
    for value in values:
        return Some(value)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    match find_mut(&mut items, "ada"):
        case Some(Item(name=n, count=k)):
            *k = 7
            print(n)?
        case Nothing:
            pass
    held = find_mut(&mut items, "bob")
    match held:
        case Some(Item(name=n, count=1) | Item(name=n, count=2)):
            *n = "bo"
        case Some(_) | Nothing:
            pass
    print(items[0].count, items[1].name)?
    mut values = [Some(1)]?
    slot = first(&mut values)
    match slot:
        case Some(Some(v)):
            *v = 4
        case Some(Nothing) | Nothing:
            pass
    print(values)?
    Ok(())
"#;
    assert_stdout(program, "ada\n7 bo\n[Option[i64].Some(4)]\n");
    rejects(
        &with_find(
            "mut items = [Item(\"ada\", 1)].unwrap()\nmatch find_mut(&mut items, \"ada\"):\n    case Some(Item(name=n, count=k)):\n        items.clear()\n        *k = 2\n        print(n).unwrap()\n    case Nothing:\n        pass\n",
        ),
        "cannot modify or exclusively borrow `items` while it is exclusively borrowed",
    );
}

/// With a borrowed payload, `map_or`, `map_or_else` and `unwrap_or_else`
/// lower as the `match` they abbreviate (issue #868): the mapper receives
/// the reference, and a reference result keeps both alternatives borrowed.
#[test]
fn borrowed_payload_adapters_lower_as_a_match() {
    let program = r#"
def name_of(item: &Item) -> &str:
    &item.name

def bump(item: &mut Item) -> i64:
    item.count = 40
    item.count

def label(items: &list[Item], key: &str) -> &str:
    find(items, key).map_or(&items[0].name, name_of)

def pick(items: &list[Item], key: &str) -> Result[&Item, str]:
    for item in items:
        if item.name == key:
            return Ok(item)
    Err("missing")

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    other = Item("zed", 0)
    print(find(&items, "bob").map_or(0, def (item: &Item) -> i64: item.count))?
    print(find(&items, "x").map_or(0, def (item: &Item) -> i64: item.count))?
    print(find(&items, "x").map_or(&other.name, name_of))?
    print(label(&items, "bob"), label(&items, "x"))?
    print(find_mut(&mut items, "bob").map_or(0, bump), items[1].count)?
    print(pick(&items, "x").map_or(7, def (item: &Item) -> i64: item.count))?
    counted = pick(&items, "x").map_or_else(def (error: str) -> i64: -1, def (item: &Item) -> i64: item.count)
    print(counted)?
    print(pick(&items, "ada").map_or_else(def (error: str) -> i64: -1, def (item: &Item) -> i64: item.count))?
    Ok(())
"#;
    assert_stdout(&with_find(program), "2\n0\nzed\nbob ada\n40 40\n7\n-1\n1\n");
    for (body, message) in [
        (
            "def name_of(item: &Item) -> &str:\n    &item.name\nmut items = [Item(\"ada\", 1)].unwrap()\nother = Item(\"bob\", 2)\nname = find(&items, \"x\").map_or(&other.name, name_of)\nitems.clear()\nprint(name).unwrap()\n",
            "`name` holds a `map_or` result, which keeps both the mapped payload and the fallback borrowed",
        ),
        (
            "def me(item: &mut Item) -> &mut Item:\n    item\nmut items = [Item(\"ada\", 1)].unwrap()\nmut spare = Item(\"bob\", 2)\nslot = find_mut(&mut items, \"x\").map_or(&mut spare, me)\nprint(spare.count).unwrap()\nslot.count = 3\n",
            "cannot read or borrow `spare.count` while `spare` is exclusively borrowed",
        ),
        (
            "def name_of(item: &Item) -> &str:\n    &item.name\ndef label(items: &list[Item], other: &Item) -> &str:\n    find(items, \"x\").map_or(&other.name, name_of)\n",
            "returned reference must originate from the first reference parameter, `items`; the default of `map_or` must also borrow from `items`",
        ),
        (
            "def name_of(item: &Item) -> &str:\n    &item.name\ndef label(other: &Item, items: &list[Item]) -> &str:\n    find(items, \"x\").map_or(&other.name, name_of)\n",
            "returned reference must originate from the first reference parameter, `other`; the mapped payload of `map_or` must also borrow from `other`",
        ),
        (
            "def pick(other: &Item, items: &list[Item]) -> &Item:\n    find(items, \"x\").unwrap_or(other)\n",
            "returned reference must originate from the first reference parameter, `other`; the value of `unwrap_or` must also borrow from `other`",
        ),
    ] {
        rejects(&with_find(body), message);
    }
}

/// A reference bound below a borrowed payload can be matched again.
#[test]
fn sub_bindings_of_a_borrowed_match_match_again() {
    let program = r#"
class Q:
    p: (i64, i64)
    n: i64

def first(qs: &mut list[Q]) -> Option[&mut Q]:
    for q in qs:
        return Some(q)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut qs = [Q((1, 2), 3)]?
    match first(&mut qs):
        case Some(Q(p=pp, n=nn)):
            match pp:
                case (l, _):
                    *l = 5
            *nn = 4
        case Nothing:
            pass
    match first(&mut qs):
        case Some(qq):
            match qq:
                case Q(p=pp, n=_):
                    match pp:
                        case (_, r):
                            *r = 6
        case Nothing:
            pass
    print(qs[0].p, qs[0].n)?
    Ok(())
"#;
    assert_stdout(program, "(5, 6) 4\n");
}

/// An `if` or `match` value may be a plain `&T` or `&mut T` from different
/// owners, joining them as `unwrap_or` does (issue #897).
#[test]
fn branch_values_produce_plain_references() {
    assert_stdout(
        &with_find(
            r#"
a = [Item("ada", 1), Item("bob", 2)].unwrap()
b = [Item("cy", 3)].unwrap()
r = match find(&a, "x"):
    case Some(item):
        item
    case Nothing:
        &b[0]
print(r.name).unwrap()
s = if len(a) > 0:
    &a[1].name
else:
    &b[0].name
print(s).unwrap()
mut m = [1, 2].unwrap()
mut n = [3].unwrap()
w = if len(a) > 0:
    &mut m[0]
else:
    &mut n[0]
*w = 9
print(m[0]).unwrap()
for k in [0, 1].unwrap():
    x = if k == 0:
        &a[0]
    else:
        &b[0]
    print(x.name).unwrap()
"#,
        ),
        "cy\nbob\n9\nada\ncy\n",
    );
    assert_stdout(
        &with_find(
            r#"
def pick(items: &list[Item], first: bool) -> &Item:
    if first:
        &items[0]
    else:
        &items[1]
a = [Item("ada", 1), Item("bob", 2)].unwrap()
print(pick(&a, False).name).unwrap()
"#,
        ),
        "bob\n",
    );
    assert_stdout(
        &with_find(
            r#"
a = [Item("ada", 1)].unwrap()
b = [Item("bob", 2)].unwrap()
o = find(&a, "x") if len(a) > 5 else find(&b, "bob")
print(o).unwrap()
mut m = [1].unwrap()
mut n = [2].unwrap()
w = &mut m[0] if len(a) > 0 else &mut n[0]
*w = 7
print(m[0]).unwrap()
"#,
        ),
        "Option[&Item].Some(Item(name=\"bob\", count=2))\n7\n",
    );
}

#[test]
fn plain_branch_references_keep_every_owner_borrowed() {
    rejects(
        &with_find(
            r#"
mut a = [Item("ada", 1)].unwrap()
b = [Item("bob", 2)].unwrap()
r = if len(a) > 5:
    &a[0]
else:
    &b[0]
a.clear()
print(r.name).unwrap()
"#,
        ),
        "`r` holds this arm's value: an `if` or `match` value keeps the value of every arm borrowed",
    );
    rejects(
        r#"
mut m = [1, 2].unwrap()
w = if len(m) > 0:
    &mut m[0]
else:
    &mut m[1]
print(m[1]).unwrap()
*w = 3
"#,
        "cannot read or borrow `m[1]` while it is exclusively borrowed",
    );
    rejects(
        &with_find(
            r#"
a = [Item("ada", 1)].unwrap()
v = if len(a) > 0:
    local = Item("tmp", 0)
    &local
else:
    &a[0]
print(v.name).unwrap()
"#,
        ),
        "cannot drop `local` while it is borrowed",
    );
    rejects(
        &with_find(
            r#"
def pick(items: &list[Item], other: &list[Item], first: bool) -> &Item:
    return if first:
        &items[0]
    else:
        &other[0]
"#,
        ),
        "returned reference must originate from the first reference parameter, `items`; this one borrows `other`",
    );
}

/// A conditional's dictionary view has no join loan, so it is rejected
/// rather than left unborrowed (local PR #1126 review).
#[test]
fn conditional_views_are_rejected() {
    const VIEW: &str = "a conditional expression cannot produce a dictionary view";
    rejects(
        r#"
mut d = {"x": [1, 2, 3].unwrap()}.unwrap()
c = len(d) > 0
r = d.values() if c else d.values()
for v in r:
    d.clear()
    print(v).unwrap()
"#,
        VIEW,
    );
    rejects(
        r#"
mut d = {"x": 1}.unwrap()
e = {"y": 2}.unwrap()
k = d.keys() if len(d) > 0 else e.keys()
d.clear()
print(len(k)).unwrap()
"#,
        VIEW,
    );
    rejects(
        r#"
mut d = {"x": [1].unwrap()}.unwrap()
for v in (d.values() if len(d) > 0 else d.values()):
    d.clear()
    print(v).unwrap()
"#,
        VIEW,
    );
}

/// A conditional may choose a closure with borrowed captures: its capture
/// loans follow the value, so the captured owner stays borrowed.
#[test]
fn conditional_borrowing_closures_keep_captures_borrowed() {
    assert_stdout(
        r#"
mut a = [1, 2].unwrap()
read = def [&a]() -> i64:
    len(a)
g = read if len(a) > 0 else read
print(g()).unwrap()
a.clear()
"#,
        "2\n",
    );
    rejects(
        r#"
mut a = [1, 2].unwrap()
read = def [&a]() -> i64:
    len(a)
g = read if len(a) > 0 else read
a.clear()
print(g()).unwrap()
"#,
        "conflicting borrow",
    );
}

/// A `&mut` payload matched out of a holder reborrows the holder's loan
/// exclusively, so the holder cannot be read until the payload's last use
/// (issue #901).
#[test]
fn an_exclusive_payload_borrows_its_holder() {
    let conflict = "cannot read `a` (through `x`) while it is exclusively borrowed";
    rejects(
        &with_find(
            r#"
mut a = [Item("ada", 1)].unwrap()
x = find_mut(&mut a, "ada")
match x:
    case Some(i):
        print(x).unwrap()
        i.count = 5
    case Nothing:
        pass
"#,
        ),
        conflict,
    );
    for value in [
        "match x:\n    case Some(i):\n        i\n    case Nothing:\n        &mut b[0]",
        "match x:\n    case Some(i):\n        Some(i)\n    case Nothing:\n        Some(&mut b[0])",
        "x.unwrap_or(&mut b[0])",
        "x.map_or(&mut b[0], def(i: &mut Item) -> &mut Item: i)",
    ] {
        let use_r = if value.contains("Some(&mut") {
            "match r:\n    case Some(j):\n        j.count = 5\n    case Nothing:\n        pass"
        } else {
            "r.count = 5"
        };
        rejects(
            &with_find(&format!(
                "mut a = [Item(\"ada\", 1)].unwrap()\nmut b = [Item(\"bob\", 2)].unwrap()\nx = find_mut(&mut a, \"ada\")\nr = {value}\nprint(x).unwrap()\n{use_r}\n"
            )),
            conflict,
        );
    }
    // Reading the holder after the payload's last use, or on a later
    // iteration after it is replaced, stays accepted.
    let program = format!(
        "{FIND}{}",
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut a = [Item("ada", 1)]?
    x = find_mut(&mut a, "ada")
    match x:
        case Some(i):
            i.count = 5
            print(x)?
        case Nothing:
            pass
    print(x)?
    mut y = find_mut(&mut a, "ada")
    while True:
        match y:
            case Some(i):
                i.count = 6
                print(y)?
                y = Nothing
            case Nothing:
                break
    Ok(())
"#
    );
    assert_stdout(
        &program,
        "Option[&mut Item].Some(Item(name=\"ada\", count=5))\n\
         Option[&mut Item].Some(Item(name=\"ada\", count=5))\n\
         Option[&mut Item].Some(Item(name=\"ada\", count=6))\n",
    );
}

/// A copy of an exclusive holder reborrows it, so the original cannot be
/// read while the copy is live (issue #901); shared copies coexist.
#[test]
fn an_exclusive_holder_copy_borrows_the_original() {
    let error = check_error(&with_find(
        "mut a = [Item(\"ada\", 1)].unwrap()\nx = find_mut(&mut a, \"ada\")\ny = x\nprint(x).unwrap()\nprint(y).unwrap()\n",
    ));
    for message in [
        "cannot read `a` (through `x`) while it is exclusively borrowed",
        "`y` copies `x` and shares its exclusive borrow, so `x` cannot be used while `y` is live; use `y` instead",
    ] {
        assert!(error.contains(message), "expected {message:?}, got {error}");
    }
    assert_stdout(
        &format!(
            "{FIND}{}",
            r#"
def main() -> Result[(), AllocError | IoError]:
    a = [Item("ada", 1)]?
    x = find(&a, "ada")
    y = x
    print(x)?
    print(y)?
    mut b = [Item("bob", 2)]?
    p = find_mut(&mut b, "bob")
    q = p
    print(q)?
    print(p)?
    Ok(())
"#
        ),
        "Option[&Item].Some(Item(name=\"ada\", count=1))\n\
         Option[&Item].Some(Item(name=\"ada\", count=1))\n\
         Option[&mut Item].Some(Item(name=\"bob\", count=2))\n\
         Option[&mut Item].Some(Item(name=\"bob\", count=2))\n",
    );
}
