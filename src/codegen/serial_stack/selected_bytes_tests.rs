//! Inspect the actual caller-side budget word without executing a codec or provider.
use super::*;

const ADAPTER: &str = r#"
import std.serial
import std.alloc as alloc
import std.bytes as binary
from std.serial import Fault

pub class Forward:
    root: alloc.Allocator
    pub def allocate(self: &mut Forward, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.root.block(request)
    pub def release(self: &mut Forward, storage: alloc.ByteBlock) -> ():
        drop(storage)

pub class Sink:
    data: binary.Bytes
    saved: Option[Box[binary.Bytes]]
    pub def boolean(self: &mut Sink, value: bool) -> Result[(), Fault]:
        CHECKED_BODY
        Ok(())
    pub def reject(self, fault: Fault) -> Fault:
        REJECT_BODY
        fault
    pub def allocation(self, cause: AllocError) -> Fault:
        Fault.OutOfRange
    pub def null(self: &mut Sink) -> Result[(), Fault]:
        Ok(())
    pub def integer(self: &mut Sink, value: i64) -> Result[(), Fault]:
        Ok(())
    pub def unsigned(self: &mut Sink, value: u64) -> Result[(), Fault]:
        Ok(())
    pub def decimal(self: &mut Sink, value: f64) -> Result[(), Fault]:
        Ok(())
    pub def text(self: &mut Sink, value: &str) -> Result[(), Fault]:
        Ok(())
    pub def start_array(self: &mut Sink) -> Result[(), Fault]:
        Ok(())
    pub def end_array(self: &mut Sink) -> Result[(), Fault]:
        Ok(())
    pub def start_object(self: &mut Sink) -> Result[(), Fault]:
        Ok(())
    pub def key(self: &mut Sink, name: &str) -> Result[(), Fault]:
        Ok(())
    pub def field(self: &mut Sink, name: &str) -> Result[(), Fault]:
        Ok(())
    pub def end_object(self: &mut Sink) -> Result[(), Fault]:
        Ok(())

def main() -> Result[(), AllocError]:
    root = alloc.system()
    PROVIDER
    mut sink = Sink(BUFFER?, Some(Box(BUFFER?)?))
    flag = True
    drop(std.serial.encode[bool, Fault, Sink](&flag, &mut sink))
    Ok(())
"#;

fn budget(selected: bool, registered: bool, checked: &str, reject: &str) -> Result<u64> {
    let source = ADAPTER
        .replace("CHECKED_BODY", checked)
        .replace("REJECT_BODY", reject)
        .replace(
            "PROVIDER",
            if registered {
                "out = alloc.allocator(&root, Forward(root.share())).unwrap()"
            } else {
                "out = root.share()"
            },
        )
        .replace(
            "BUFFER",
            if selected {
                "out.slice_bytes(b\"x\")"
            } else {
                "binary.from_bytes(b\"x\")"
            },
        );
    let mut heap = Heap::default();
    let program = crate::frontend::compile(&source, &mut heap)?;
    let out = object_module(
        program.ops,
        &heap,
        Some(program.returns_status),
        &[],
        None,
        1,
        Build {
            optimize: true,
            ..Build::default()
        },
    )?;
    let requests = out.serial_stack.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "one checked call in main");
    let id = *requests.values().next().unwrap();
    drop(requests);
    let product = out.finish()?;
    let symbol = product.object.symbol(product.data_symbol(id));
    let section = product.object.section(symbol.section.id().unwrap());
    let start = symbol.value as usize;
    Ok(u64::from_le_bytes(
        section.data()[start..start + 8].try_into()?,
    ))
}

#[test]
fn push_through_borrowed_adapter_requires_both_emitted_facts() {
    let push = "self.data.push(1u8).unwrap()";
    for selected in [false, true] {
        for registered in [false, true] {
            let required = budget(selected, registered, push, "pass").unwrap();
            assert_eq!(
                required == u64::MAX,
                selected && registered,
                "selected={selected}, registered={registered}"
            );
        }
    }
}

#[test]
fn reference_operations_distinguish_owned_temporaries_from_loans() {
    let functions = HashMap::default();
    let targets = HashMap::default();
    let owned = crate::sum::option(Ty::Box(Arc::new(Ty::ByteStorage)));
    let loan = crate::sum::option(Ty::Ref(Arc::new(Ty::ByteStorage), true));
    for operation in [Op::ReadRef, Op::ReplaceRef, Op::WriteRef] {
        for (mixed, ty, unknown) in [
            (true, &owned, true),
            (false, &owned, false),
            (true, &loan, false),
        ] {
            let mut graph = Graph::new(&functions, &[], &targets, ByteProviders(mixed));
            graph.body(&[operation(ty.clone())]);
            assert_eq!(graph.unknown, unknown, "mixed={mixed}, ty={ty}");
        }
    }
}

#[test]
fn mixed_program_rejects_growing_byte_methods_but_not_an_in_place_indexed_write() {
    for body in [
        "self.data.reserve(1).unwrap()",
        "self.data.resize(9, 0u8).unwrap()",
        "self.data.extend(b\"more\").unwrap()",
    ] {
        assert_eq!(
            budget(true, true, body, "pass").unwrap(),
            u64::MAX,
            "{body}"
        );
    }
    assert_ne!(
        budget(true, true, "self.data[0] = 0u8", "pass").unwrap(),
        u64::MAX
    );
}

#[test]
fn replacing_an_owned_payload_through_a_borrow_requires_a_budget_for_its_release() {
    let clear = "self.saved = Nothing";
    assert_ne!(budget(false, true, clear, "pass").unwrap(), u64::MAX);
    assert_eq!(budget(true, true, clear, "pass").unwrap(), u64::MAX);
}

#[test]
fn mixed_program_keeps_borrowed_indexing_and_requires_a_bounded_reject_bridge() {
    let read = "drop(self.data[0])";
    assert_ne!(budget(true, true, read, "pass").unwrap(), u64::MAX);
    // Dropping the Result may destroy an owned Bytes. Even this freshly
    // ordinary buffer isn't distinguished by its type in a mixed program.
    let reject = "drop(binary.from_bytes(b\"x\"))";
    assert_ne!(budget(true, false, read, reject).unwrap(), u64::MAX);
    let error = budget(true, true, read, reject).unwrap_err().to_string();
    assert!(
        error.contains("cannot safely report serialization stack exhaustion"),
        "{error}"
    );
}
