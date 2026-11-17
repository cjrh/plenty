//! Compiled provider registration, callback ownership and admission.
mod support;
use super::test_std_alloc::assert_stdout;

#[test]
fn registered_provider_retains_state_for_lists_and_blocks() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Policy:
    parent: alloc.Allocator
    id: i64
    payload: array[i64, 8]
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        for i in range(storage.len()):
            if storage.get(i) != 0u8:
                print("not zero").unwrap()
        drop(storage)
    def __del__(self: &mut Policy) -> ():
        print("policy", self.id, self.payload[7]).unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(), 42, array[i64, 8].filled(17))).unwrap()
    mut values = owner.empty_list[i64]()?
    block = owner.block(&alloc.layout(9).unwrap())?
    drop(owner)
    for i in range(40):
        values.append(i)?
    print(len(values), values[39], block.len())?
    drop(values)
    print("block alive")?
    drop(block)
    print("done")?
    Ok(())
"#,
        "40 39 9\nblock alive\npolicy 42 17\ndone\n",
    );
}

#[test]
fn registration_failure_returns_generic_inline_provider_at_both_bootstrap_points() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Budget:
    parent: alloc.Allocator
    left: i64
    calls: i64
    pub def allocate(self: &mut Budget, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        unchecked:
            self.calls += 1
        if self.left == 0:
            return Err(AllocError.OutOfMemory)
        unchecked:
            self.left -= 1
        self.parent.block(request)
    pub def release(self: &mut Budget, storage: alloc.ByteBlock) -> ():
        print("bootstrap release", storage.len() > 0).unwrap()
        drop(storage)
    def __del__(self: &mut Budget) -> ():
        print("bootstrap calls", self.calls).unwrap()

pub class Child:
    id: i64
    def __del__(self: &mut Child) -> ():
        print("child", self.id).unwrap()

pub class Policy[T]:
    parent: alloc.Allocator
    values: array[i64, 9]
    child: T
    pub def allocate(self: &mut Policy[T], request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.parent.block(request)
    pub def release(self: &mut Policy[T], storage: alloc.ByteBlock) -> ():
        drop(storage)
    def __del__(self: &mut Policy[T]) -> ():
        print("policy", self.values[8]).unwrap()

def main() -> Result[(), AllocError | IoError]:
    root = alloc.system()
    for budget in range(3):
        bootstrap = alloc.allocator(&root, Budget(root.share(), budget, 0)).unwrap()
        policy = Policy(root.share(), array[i64, 9].filled(budget), Box(Child(budget))?)
        owner = match alloc.allocator(&bootstrap, policy):
            case Ok(owner):
                print("registered")?
                owner
            case Err(alloc.RegistrationError.Allocation(original, error)):
                print("returned", original.values[8], error)?
                alloc.allocator(&root, original).unwrap()
            case Err(alloc.RegistrationError.UnsupportedContext(original)):
                drop(original)
                return Ok(())
        drop(bootstrap)
        block = owner.block(&alloc.layout(7).unwrap())?
        print("block", block.len())?
        drop(owner)
        drop(block)
    Ok(())
"#,
        "returned 0 AllocError.OutOfMemory\nbootstrap calls 1\nblock 7\npolicy 0\nchild 0\nbootstrap release True\nreturned 1 AllocError.OutOfMemory\nbootstrap calls 2\nblock 7\npolicy 1\nchild 1\nregistered\nblock 7\npolicy 2\nchild 2\nbootstrap release True\nbootstrap release True\nbootstrap calls 2\n",
    );
}

#[test]
fn independent_provider_budgets_and_errors_never_fall_back() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Budget:
    parent: alloc.Allocator
    left: i64
    error: AllocError
    pub def allocate(self: &mut Budget, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        if self.left == 0:
            return Err(self.error)
        unchecked:
            self.left -= 1
        self.parent.block(request)
    pub def release(self: &mut Budget, storage: alloc.ByteBlock) -> ():
        drop(storage)

def main() -> Result[(), IoError]:
    root = alloc.system()
    a = alloc.allocator(&root, Budget(root.share(), 0, AllocError.OutOfMemory)).unwrap()
    b = alloc.allocator(&root, Budget(root.share(), 1, AllocError.CapacityOverflow)).unwrap()
    request = alloc.layout(8).unwrap()
    print(a.block(&request).is_err(), b.block(&request).is_ok())?
    match b.block(&request):
        case Err(error):
            print(error)?
        case Ok(block):
            drop(block)
    print(a.empty_list[i64]().is_err())?
    print(root.block(&request).is_ok())?
    Ok(())
"#,
        "True True\nAllocError.CapacityOverflow\nTrue\nTrue\n",
    );
}

#[test]
fn provider_callbacks_and_final_managed_state_destruction_run_on_worker() {
    assert_stdout(
        r#"import std.alloc as alloc

pub class Child:
    id: i64
    def __del__(self: &mut Child) -> ():
        print("child", self.id).unwrap()

pub class Policy:
    parent: alloc.Allocator
    child: Box[Child]
    calls: i64
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        unchecked:
            self.calls += 1
        self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
    def __del__(self: &mut Policy) -> ():
        print("calls", self.calls).unwrap()

def work(owner: alloc.Allocator) -> ():
    block = owner.block(&alloc.layout(17).unwrap()).unwrap()
    print("worker", block.len()).unwrap()
    drop(block)
    drop(owner)
    print("worker done").unwrap()

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(), Box(Child(9))?, 0)).unwrap()
    with spawn(work, owner).drop_job()? as task:
        task.join()
    print("joined")?
    Ok(())
"#,
        "worker 17\ncalls 1\nchild 9\nworker done\njoined\n",
    );
}

#[test]
fn provider_bridge_intrinsics_are_private() {
    for intrinsic in [
        "__allocator_register",
        "__allocator_commit",
        "__allocator_block_take",
    ] {
        let error = support::check_error(&format!("def main() -> ():\n    drop({intrinsic}())\n"));
        assert!(
            error.contains("reserved for the standard library"),
            "{error}"
        );
    }
}

fn assert_fatal(source: &str, message: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    for optimize in [false, true] {
        plenty::compile_source_to_executable_with_options(
            source,
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap();
        let output = support::output_within(
            &mut std::process::Command::new(&executable),
            b"",
            std::time::Duration::from_secs(15),
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
        assert!(
            stderr.contains("allocator callback traps are process-fatal"),
            "{stderr}"
        );
    }
}

#[test]
fn provider_traps_are_fatal_in_allocate_release_destroy_and_transitive_cleanup() {
    for mode in [
        "allocate",
        "release",
        "destroy",
        "child_allocate",
        "child_release",
        "child_destroy",
    ] {
        let trap = "empty: Option[i64] = Nothing\n        drop(empty.unwrap())";
        let allocating = match mode {
            "allocate" => format!("{trap}\n        self.parent.block(request)"),
            "child_allocate" => "child = Box(Child()).unwrap()\n        drop(child)\n        self.parent.block(request)".to_owned(),
            _ => "self.parent.block(request)".to_owned(),
        };
        let releasing = match mode {
            "release" => format!("{trap}\n        drop(storage)"),
            "child_release" => {
                "child = Box(Child()).unwrap()\n        drop(child)\n        drop(storage)"
                    .to_owned()
            }
            _ => "drop(storage)".to_owned(),
        };
        let child_field = if mode == "child_destroy" {
            "    child: Box[Child]\n"
        } else {
            ""
        };
        let child_argument = if mode == "child_destroy" {
            ", Box(Child()).unwrap()"
        } else {
            ""
        };
        let destructor = if mode == "destroy" { trap } else { "pass" };
        let source = format!(
            r#"import std.alloc as alloc
pub class Child:
    def __del__(self: &mut Child) -> ():
        {trap}

pub class Policy:
    parent: alloc.Allocator
{child_field}    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        {allocating}
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        {releasing}
    def __del__(self: &mut Policy) -> ():
        {destructor}

def main() -> ():
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(){child_argument})).unwrap()
    block = owner.block(&alloc.layout(8).unwrap()).unwrap()
    drop(block)
    # Final provider cleanup runs while another managed owner's queue drains.
    holder = Box(owner).unwrap()
    drop(holder)
"#
        );
        assert_fatal(&source, "unwrap");
    }
}

#[test]
fn invalid_provider_extent_and_alignment_trap_before_typed_writes() {
    for allocation in [
        "self.parent.block(&alloc.layout(0).unwrap())",
        "mut block = self.parent.block(&alloc.layout(1024, 64).unwrap())?\n        Ok(block.split_off(1))",
    ] {
        assert_fatal(
            &format!(
                r#"import std.alloc as alloc
pub class Policy:
    parent: alloc.Allocator
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        {allocation}
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> ():
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share())).unwrap()
    drop(owner.block(&alloc.layout(8, 64).unwrap()))
"#
            ),
            "allocator provider returned insufficient extent or alignment",
        );
    }
}

#[test]
fn provider_admission_rejects_transitive_effects_and_opaque_state_without_spawn() {
    for mode in ["allocate", "release", "destroy", "state"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("native.plentyi"),
            r#"pub opaque Pointer
pub extern def pointer() -> Pointer = "pointer"
pub extern def unsafe_call() -> () = "unsafe_call"
"#,
        )
        .unwrap();
        // Reachability through an imported ordinary helper is still certified.
        std::fs::write(
            root.path().join("helper.plenty"),
            "import native\npub def call() -> ():\n    native.unsafe_call()\n",
        )
        .unwrap();
        let field = if mode == "state" {
            "    pointer: native.Pointer\n"
        } else {
            ""
        };
        let argument = if mode == "state" {
            ", native.pointer()"
        } else {
            ""
        };
        let allocate = if mode == "allocate" {
            "helper.call()\n        "
        } else {
            ""
        };
        let release = if mode == "release" {
            "helper.call()\n        "
        } else {
            ""
        };
        let destroy = if mode == "destroy" {
            "helper.call()"
        } else {
            "pass"
        };
        let path = root.path().join("main.plenty");
        std::fs::write(&path, format!(r#"import std.alloc as alloc
import helper
import native
pub class Policy:
    parent: alloc.Allocator
{field}    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        {allocate}self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        {release}drop(storage)
    def __del__(self: &mut Policy) -> ():
        {destroy}
def main() -> ():
    root = alloc.system()
    drop(alloc.allocator(&root, Policy(root.share(){argument})))
"#)).unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains("allocator provider"), "{mode}: {error}");
        assert!(
            error.contains(if mode == "state" {
                "no thread contract"
            } else {
                "no worker-thread effect contract"
            }),
            "{mode}: {error}"
        );
    }
}

#[test]
fn provider_protocol_requires_public_mutable_callbacks_and_borrowed_layout() {
    for (allocate_receiver, request, visibility) in [
        ("&Policy", "&alloc.Layout", "pub "),
        ("&mut Policy", "alloc.Layout", "pub "),
        ("&mut Policy", "&alloc.Layout", ""),
    ] {
        let source = format!(
            r#"import std.alloc as alloc
pub class Policy:
    parent: alloc.Allocator
    {visibility}def allocate(self: {allocate_receiver}, request: {request}) -> Result[alloc.ByteBlock, AllocError]:
        self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> ():
    root = alloc.system()
    drop(alloc.allocator(&root, Policy(root.share())))
"#
        );
        let error = support::check_error(&source);
        assert!(error.contains("allocate"), "{error}");
        assert!(
            error.contains("does not satisfy") || error.contains("private"),
            "{error}"
        );
    }
}

#[test]
fn provider_state_cannot_hide_channel_ownership() {
    let error = support::check_error(
        r#"import std.alloc as alloc
pub class Policy:
    sender: Sender[i64]
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        alloc.system().block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    root = alloc.system()
    drop(alloc.allocator(&root, Policy(sender)))
    drop(receiver)
"#,
    );
    assert!(
        error.contains("allocator provider state")
            && error.contains("hidden task/channel ownership"),
        "{error}"
    );
}

#[test]
fn provider_host_callback_permission_is_not_a_worker_contract() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("native.plentyi"),
        r#"@thread_shareable
pub opaque Pointer
@host_callback
pub extern def call(function: Pointer) -> () = function
@thread_safe
pub extern def pointer() -> Pointer = "native_pointer"
"#,
    )
    .unwrap();
    let path = root.path().join("main.plenty");
    std::fs::write(&path, r#"import std.alloc as alloc
import native
pub class Policy:
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        native.call(native.pointer())
        alloc.system().block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> ():
    root = alloc.system()
    drop(alloc.allocator(&root, Policy()))
"#).unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(
        error.contains("allocator provider") && error.contains("foreign host entry context"),
        "{error}"
    );
}

#[test]
fn copyable_provider_state_commits_retained_fields_without_leaking_or_double_release() {
    assert_stdout(
        r#"import std.alloc as alloc
@copy
pub class Policy:
    label: str
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        print(self.label).unwrap()
        alloc.system().block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> Result[(), IoError]:
    root = alloc.system()
    label = str.from(42).unwrap()
    source = Policy(label)
    owner = alloc.allocator(&root, source).unwrap()
    drop(label)
    print(source.label)?
    drop(source)
    block = owner.block(&alloc.layout(1).unwrap()).unwrap()
    drop(owner)
    drop(block)
    Ok(())
"#,
        "42\n42\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn compiled_registration_uses_no_implicit_metadata_allocation() {
    for successes in 0..=2 {
        assert_stdout(
            &format!(
                r#"import std.alloc as alloc
pub class Child:
    def __del__(self: &mut Child) -> ():
        print("child").unwrap()
pub class Policy:
    child: Box[Child]
    payload: array[i64, 9]
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        alloc.system().block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
    def __del__(self: &mut Policy) -> ():
        print("policy", self.payload[8]).unwrap()
def main() -> Result[(), IoError]:
    root = alloc.system()
    policy = Policy(Box(Child()).unwrap(), array[i64, 9].filled(73))
    print("__test_fail_allocations_after_{successes}__")?
    result = alloc.allocator(&root, policy)
    print("__test_restore_allocations__")?
    print(result.is_ok())?
    drop(result)
    Ok(())
"#
            ),
            &format!(
                "__test_fail_allocations_after_{successes}__\n__test_restore_allocations__\n{}\npolicy 73\nchild\n",
                if successes == 2 { "True" } else { "False" }
            ),
        );
    }
}

#[test]
fn safe_shared_state_reentrancy_traps_instead_of_deadlocking() {
    assert_fatal(
        r#"import std.alloc as alloc
from std.sync import Mutex
pub class Policy:
    slot: Shared[Mutex[Option[alloc.Allocator]]]
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        with self.slot.lock() as owner:
            match owner:
                case Some(handle):
                    return handle.block(request)
                case Nothing:
                    return Err(AllocError.OutOfMemory)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> ():
    slot = Shared(Mutex[Option[alloc.Allocator]](Nothing)).unwrap()
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(slot.share())).unwrap()
    with slot.lock() as value:
        *value = Some(owner.share())
    drop(owner.block(&alloc.layout(8).unwrap()))
"#,
        "reentrant allocator callback",
    );
}

#[test]
fn provider_mutable_callbacks_are_serialized_across_workers() {
    assert_stdout(
        r#"import std.alloc as alloc
pub class Policy:
    parent: alloc.Allocator
    calls: i64
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        unchecked:
            self.calls += 1
        self.parent.block(request)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
    def __del__(self: &mut Policy) -> ():
        print(self.calls).unwrap()
def work(owner: alloc.Allocator) -> ():
    request = alloc.layout(8).unwrap()
    for i in range(100):
        drop(owner.block(&request).unwrap())
def main() -> Result[(), SpawnError[()]]:
    root = alloc.system()
    owner = alloc.allocator(&root, Policy(root.share(), 0)).unwrap()
    with spawn(work, owner.share()).drop_job()? as first, spawn(work, owner.share()).drop_job()? as second:
        first.join()
        second.join()
    drop(owner)
    Ok(())
"#,
        "200\n",
    );
}

#[test]
fn enum_provider_state_uses_box_slot_reference_and_preserves_bootstrap_capacity_error() {
    assert_stdout(
        r#"import std.alloc as alloc
pub enum Policy:
    Parent(alloc.Allocator)
    Exhausted
    pub def allocate(self: &mut Policy, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        match self:
            case Policy.Parent(parent):
                parent.block(request)
            case Policy.Exhausted:
                Err(AllocError.CapacityOverflow)
    pub def release(self: &mut Policy, storage: alloc.ByteBlock) -> ():
        drop(storage)
def main() -> Result[(), IoError]:
    root = alloc.system()
    exhausted = alloc.allocator(&root, Policy.Exhausted).unwrap()
    source = Policy.Parent(root.share())
    owner = match alloc.allocator(&exhausted, source):
        case Ok(owner):
            owner
        case Err(alloc.RegistrationError.Allocation(original, error)):
            print(error)?
            alloc.allocator(&root, original).unwrap()
        case Err(alloc.RegistrationError.UnsupportedContext(original)):
            drop(original)
            return Ok(())
    block = owner.block(&alloc.layout(3).unwrap()).unwrap()
    drop(owner)
    print(block.len())?
    Ok(())
"#,
        "AllocError.CapacityOverflow\n3\n",
    );
}
