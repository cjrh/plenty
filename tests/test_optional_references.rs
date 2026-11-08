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
fn mut_holders_stay_within_their_first_place() {
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
    rejects(
        &with_find(
            r#"
a = [Item("ada", 1)].unwrap()
b = [Item("bob", 1)].unwrap()
mut pick: Option[&Item] = Nothing
if len(a) > 0:
    pick = Some(&a[0])
else:
    pick = Some(&b[0])
print(pick.is_some()).unwrap()
"#,
        ),
        "holder `pick` borrows another owner, since its first borrowing assignment at line 22; it can be assigned only borrows of that owner, and assigning `Nothing` does not release it",
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
