use super::*;

/// These fixtures deliberately mark checked functions as embedded code:
/// real std algorithms should not gain contrived callback-then-trap paths
/// merely to exercise diagnostic context restoration.
fn fixture(source: &str) -> String {
    let mut heap = Heap::default();
    let mut program = crate::frontend::compile(source, &mut heap).unwrap();
    for op in &mut program.ops {
        if let Op::DefineFn(name, f) = op {
            let origins = Arc::make_mut(&mut f.origins);
            origins.standard_library = name.starts_with("standard_");
            origins.trap_file = Some(if origins.standard_library {
                "std/fixture.plenty".into()
            } else {
                "user.plenty".into()
            });
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    compile_ops_to_executable(
        program.ops,
        &heap,
        &executable,
        program.returns_status,
        &crate::CompileOptions::default(),
        false,
    )
    .unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "program terminated with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn nested_standard_helpers_restore_the_caller_after_a_tail_calling_callback() {
    let error = fixture(
        "def standard_identity(n: i64) -> i64:\n    n\ndef callback(n: i64) -> i64:\n    standard_identity(n)\ndef standard_forward(n: i64) -> i64:\n    callback(n)\ndef standard_inner(n: i64) -> i64:\n    n // 0\ndef standard_outer(n: i64) -> i64:\n    value = standard_forward(n)\n    standard_inner(value)\ndef main() -> Result[(), IoError]:\n    print(standard_outer(1))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:8:5: division by zero\nnote: called from user.plenty:13:11\n"
    );
}

#[test]
fn a_standard_generator_reports_the_resume_site() {
    let error = fixture(
        "def standard_values() -> Generator[i64]:\n    yield 1\n    yield 1 // 0\ndef main() -> Result[(), IoError]:\n    for n in standard_values():\n        print(n)?\n    Ok(())\n",
    );
    assert!(
        error.starts_with("error: std/fixture.plenty:3:11: division by zero\n"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from user.plenty:5:14\n"),
        "{error}"
    );
}

/// Issue #366: std calls to context-preserving callees save nothing, so these
/// cases check that every path that can still change the context restores it.
#[test]
fn nested_preserving_standard_helpers_keep_the_user_caller() {
    let error = fixture(
        "def standard_inner(n: i64) -> i64:\n    n // 0\ndef standard_count(n: i64, total: i64) -> i64:\n    if n == 0:\n        return total\n    standard_count(n - 1, total + n)\ndef standard_mid(n: i64) -> i64:\n    value = standard_count(n, 0)\n    standard_inner(value)\ndef standard_outer(n: i64) -> i64:\n    value = standard_mid(n)\n    value + 1\ndef main() -> Result[(), IoError]:\n    print(standard_outer(3))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:2:5: division by zero\nnote: called from user.plenty:14:11\n"
    );
}

#[test]
fn a_standard_trap_after_an_indirect_callback_returns_names_the_original_caller() {
    let error = fixture(
        "def standard_identity(n: i64) -> i64:\n    n\ndef callback(n: i64) -> i64:\n    standard_identity(n)\ndef standard_inner(n: i64) -> i64:\n    n // 0\ndef standard_apply(f: &BoxedCallable[[i64], i64], n: i64) -> i64:\n    value = f(n)\n    standard_inner(value)\ndef main() -> Result[(), AllocError | IoError]:\n    g = BoxedCallable(callback)?\n    print(standard_apply(&g, 1))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:6:5: division by zero\nnote: called from user.plenty:12:11\n"
    );
}

#[test]
fn a_standard_trap_inside_a_callback_names_the_callback() {
    let error = fixture(
        "def standard_inner(n: i64) -> i64:\n    n // 0\ndef callback(n: i64) -> i64:\n    standard_inner(n) + 1\ndef standard_apply(f: &BoxedCallable[[i64], i64], n: i64) -> i64:\n    f(n)\ndef main() -> Result[(), AllocError | IoError]:\n    g = BoxedCallable(callback)?\n    print(standard_apply(&g, 1))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:2:5: division by zero\nnote: called from user.plenty:4:5\n"
    );
}

#[test]
fn a_destructor_that_tail_calls_std_does_not_leak_its_site() {
    let error = fixture(
        "def standard_touch(n: i64) -> ():\n    m = n + 1\nclass Guard:\n    n: i64\n    def __del__(self) -> ():\n        standard_touch(self.n)\ndef standard_make(n: i64) -> i64:\n    guard = Guard(n)\n    guard.n\ndef standard_outer(n: i64) -> i64:\n    value = standard_make(n)\n    value // 0\ndef main() -> Result[(), IoError]:\n    print(standard_outer(1))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:12:5: division by zero\nnote: called from user.plenty:14:11\n"
    );
}

#[test]
fn a_sort_key_that_tail_calls_std_does_not_leak_its_site() {
    let error = fixture(
        "def standard_identity(n: i64) -> i64:\n    n\ndef key(n: &i64) -> i64:\n    standard_identity(*n)\ndef standard_sort(n: i64) -> Result[i64, AllocError]:\n    mut values = [3, 1, 2]?\n    values.sort_by_key(key)\n    Ok(values[0] // (n - n))\ndef main() -> Result[(), AllocError | IoError]:\n    print(standard_sort(1)?)?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:8:8: division by zero\nnote: called from user.plenty:10:11\n"
    );
}

#[test]
fn a_standard_trap_after_advancing_a_user_generator_names_the_original_caller() {
    let error = fixture(
        "def standard_identity(n: i64) -> i64:\n    n\ndef values() -> Generator[i64]:\n    yield standard_identity(1)\ndef standard_sum(n: i64) -> i64:\n    mut total = 0\n    for v in values():\n        total += v\n    total // (total - n)\ndef main() -> Result[(), IoError]:\n    print(standard_sum(1))?\n    Ok(())\n",
    );
    assert_eq!(
        error,
        "error: std/fixture.plenty:9:5: division by zero\nnote: called from user.plenty:11:11\n"
    );
}

#[test]
fn arm_source_table_aliases_preserve_record_and_file_offsets() {
    use cranelift_object::object::{SymbolScope, write::SymbolSection};
    use object::{Object, ObjectSection, ObjectSymbol};

    for target in crate::target::NATIVE_TARGETS
        .iter()
        .filter(|target| target.arch == "aarch64")
    {
        let isa = super::super::baseline_isa(target.triple, "backtracking", "none", false).unwrap();
        let mut module = ObjectModule::new(
            cranelift_object::ObjectBuilder::new(
                isa,
                "site-aliases",
                cranelift_module::default_libcall_names(),
            )
            .unwrap(),
        );
        let mut table = Table::default();
        let records = [
            (Some("main.plenty".into()), 3, 5, false),
            (Some("std/picking.plenty".into()), 2, 5, true),
            (Some("main.plenty".into()), 7, 9, false),
        ];
        let mut aliases = Vec::new();
        for (i, record) in records.iter().enumerate() {
            let (data, offset) = table
                .append(&mut module, std::slice::from_ref(record))
                .unwrap();
            assert_eq!(offset, (i * RECORD) as i64);
            let (alias, addend) = table.address(&mut module, data, offset).unwrap();
            assert_eq!(addend, 0);
            assert_eq!(alias == data, i == 0);
            aliases.push(alias);
        }
        let placed = table.define(&mut module).unwrap();
        let mut product = module.finish();
        placed.resolve(&mut product).unwrap();
        let first = aliases[0];
        let table_symbol = product.object.symbol(product.data_symbol(first));
        let section = table_symbol.section;
        let base = table_symbol.value;
        let mut names = Vec::new();
        for (i, alias) in aliases.into_iter().enumerate() {
            let symbol = product.object.symbol(product.data_symbol(alias));
            assert!(matches!(symbol.section, SymbolSection::Section(_)));
            assert_eq!(symbol.section, section);
            assert_eq!(symbol.value, base + (i * RECORD) as u64);
            assert_eq!(symbol.scope, SymbolScope::Compilation);
            names.push(symbol.name.clone());
        }
        let bytes = product.emit().unwrap();
        let object = object::File::parse(bytes.as_slice()).unwrap();
        assert_eq!(object.format(), target.object_format);
        let symbols: Vec<_> = names
            .iter()
            .map(|name| {
                object
                    .symbols()
                    .find(|symbol| symbol.name_bytes().unwrap() == name)
                    .unwrap()
            })
            .collect();
        let section = object
            .section_by_index(symbols[0].section_index().unwrap())
            .unwrap();
        let bytes = section.data().unwrap();
        for (i, (file, line, column, standard_library)) in records.iter().enumerate() {
            let symbol = &symbols[i];
            let offset = usize::try_from(symbol.address() - section.address()).unwrap();
            let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            assert_eq!(word(offset), *line);
            assert_eq!(word(offset + 4), *column);
            assert_eq!(word(offset + 12), u32::from(*standard_library));
            let name_offset = offset + word(offset + 8) as usize;
            let name_length = word(name_offset) as usize;
            assert_eq!(
                &bytes[name_offset + 4..name_offset + 4 + name_length],
                file.as_ref().unwrap().as_bytes()
            );
            if object.format() == object::BinaryFormat::MachO && i != 0 {
                // A linker must retain these addresses in the table's atom:
                // trap entries use numeric offsets from its first record,
                // and file names use relocation-free offsets from each record.
                assert!(
                    matches!(symbol.flags(), object::SymbolFlags::MachO { n_desc, .. }
                    if n_desc.contains(object::macho::N_ALT_ENTRY)),
                    "{}",
                    target.triple
                );
            }
        }
    }
}
