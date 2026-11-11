//! Omit unused definitions according to the native artifact contract.
//!
//! Checking has already visited every body. Objects, libraries and dumps keep
//! user definitions and generators as roots, preserving their inspection/debug
//! contract without reconstructing implicit type-driven generator references. Temporary COFF executable objects instead
//! start at the entry operations and C exports: COFF resolves undefined imports
//! even in unused functions, unlike ELF/Mach-O linker garbage collection.
//!
//! A class's `__del__` is live when a live body arms it (`ClassOp::ArmDrop`,
//! issue #506). Release runs the hook only for an instance whose state word
//! was armed, and only `ArmDrop` arms one, so this covers every path that can
//! destroy an instance: fields, containers, payloads, captures, messages, and
//! jobs all hold values some live body constructed. A descriptor of an
//! unarmed class can still be emitted; it then has no hook.
use super::*;
use crate::hash::HashSet;

#[cfg(test)]
pub(super) fn prune(ops: &mut Vec<Op>, exports: &[crate::exports::Export]) {
    prune_for_emission(ops, exports, false);
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn prune_for_emission(
    ops: &mut Vec<Op>,
    exports: &[crate::exports::Export],
    executable: bool,
) {
    let candidates: HashMap<&str, &crate::op::CompiledFn> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f)
                if executable || f.origins.standard_library && f.generator.is_none() =>
            {
                Some((name.as_str(), f.as_ref()))
            }
            _ => None,
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let mut pending = Vec::new();
    let mut live = HashSet::default();
    let mut require = |name: &str| {
        if let Some((&name, _)) = candidates.get_key_value(name)
            && live.insert(name)
        {
            pending.push(name);
        }
    };
    for export in exports {
        require(&export.function);
    }
    for op in ops.iter() {
        if matches!(op, Op::DefineFn(name, _) if candidates.contains_key(name.as_str())) {
            continue;
        }
        references(std::slice::from_ref(op), &mut require);
    }
    while let Some(name) = pending.pop() {
        let function = candidates[name];
        let mut require = |name: &str| {
            if let Some((&name, _)) = candidates.get_key_value(name)
                && live.insert(name)
            {
                pending.push(name);
            }
        };
        references(&function.body, &mut require);
        if executable {
            generator_references(function, &mut require);
        }
    }
    // Keep input order. Declaration order controls object symbol numbering.
    let dead: HashSet<String> = candidates
        .keys()
        .filter(|name| !live.contains(**name))
        .map(|name| (*name).to_owned())
        .collect();
    ops.retain(|op| !matches!(op, Op::DefineFn(name, _) if dead.contains(name)));
}

/// Concrete frames can be passed or dropped without an explicit resume call.
/// Their constructor owns the resume callback and frame descriptors. Preserve
/// those identities through nested stored types of every live function.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn generator_references(function: &crate::op::CompiledFn, require: &mut impl FnMut(&str)) {
    let mut pending: Vec<_> = function
        .sig
        .inputs
        .iter()
        .map(|(_, ty)| ty.clone())
        .chain(function.sig.outputs.iter().cloned())
        .chain(function.locals.iter().cloned())
        .collect();
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        if let Ty::Generator(generator) = &ty
            && generator.members.is_empty()
            && let Some(name) = &generator.name
        {
            require(name);
        }
        crate::threading::contained(&ty, &mut pending);
    }
}

fn references(ops: &[Op], require: &mut impl FnMut(&str)) {
    // Executable emission follows concrete generator identities separately;
    // inspectable artifacts retain all generator definitions as roots.
    crate::threading::walk(ops, &mut |op| {
        crate::threading::function_references(op, require);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{CallableSig, CompiledFn, FnSig};

    fn definition(name: &str, standard: bool, body: Vec<Op>) -> Op {
        Op::DefineFn(
            name.into(),
            Box::new(CompiledFn {
                location: None,
                origins: Arc::new(crate::ownership::Origins {
                    standard_library: standard,
                    ..Default::default()
                }),
                generator: None,
                sig: Arc::new(FnSig {
                    inputs: vec![],
                    outputs: vec![],
                }),
                doc: "".into(),
                body: body.into(),
                locals: Arc::default(),
            }),
        )
    }

    fn names(ops: &[Op]) -> Vec<&str> {
        ops.iter()
            .filter_map(|op| match op {
                Op::DefineFn(name, _) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn removes_unreachable_cycles_and_keeps_transitive_calls_in_source_order() {
        let mut ops = vec![
            definition("std.live", true, vec![Op::TailCall("std.child".into())]),
            definition("std.dead", true, vec![Op::Call("std.other".into())]),
            definition("std.other", true, vec![Op::Call("std.dead".into())]),
            definition("std.child", true, vec![Op::Call("std.live".into())]),
            definition("main", false, vec![Op::Call("std.live".into())]),
        ];
        prune(&mut ops, &[]);
        assert_eq!(names(&ops), ["std.live", "std.child", "main"]);
    }

    #[test]
    fn preserves_user_bodies_implicit_roots_and_exports() {
        let mut generator = definition("std.generator", true, vec![Op::Call("std.helper".into())]);
        let Op::DefineFn(_, f) = &mut generator else {
            unreachable!()
        };
        f.generator = Some(Ty::I64);
        let mut ops = vec![
            definition("std.user_name", false, vec![]),
            generator,
            definition("std.helper", true, vec![]),
            definition("std.export", true, vec![]),
            definition("std.dead", true, vec![]),
        ];
        let exports = [crate::exports::Export {
            thread_safe: false,
            host_context: false,
            exact: false,
            function: "std.export".into(),
            name: "export".into(),
            symbol: "export".into(),
            signature: Arc::new(FnSig {
                inputs: vec![],
                outputs: vec![],
            }),
            documentation: String::new(),
        }];
        prune(&mut ops, &exports);
        assert_eq!(
            names(&ops),
            ["std.user_name", "std.generator", "std.helper", "std.export"]
        );
    }

    fn armed(name: &str) -> Op {
        Op::Class(crate::record::ClassOp::ArmDrop(
            crate::nominal::Nominal::new(crate::record::ClassType {
                name: name.into(),
                fields: vec![],
                destructor: Some(crate::record::method(name, "__del__")),
                fallible_init: false,
                copy: false,
                forwards_operators: false,
                redacted: false,
                quantity: None,
                bounds: None,
                bytes: Default::default(),
                managed: Default::default(),
            }),
        ))
    }

    #[test]
    fn keeps_a_standard_destructor_only_when_a_live_body_arms_its_class() {
        let destructor = |class: &str, helper: &str| {
            definition(
                &crate::record::method(class, "__del__"),
                true,
                vec![Op::Call(helper.into())],
            )
        };
        let mut ops = vec![
            destructor("std.Armed", "std.armed_helper"),
            destructor("std.Unused", "std.unused_helper"),
            destructor("std.Nested", "std.nested_helper"),
            // Arming happens in a std constructor reached only from `main`,
            // inside control flow; the armed destructor arms another class.
            definition(
                "std.make",
                true,
                vec![Op::If {
                    then: vec![armed("std.Armed")].into(),
                    elifs: Arc::default(),
                    otherwise: Arc::default(),
                }],
            ),
            definition("std.unused_make", true, vec![armed("std.Unused")]),
            definition("std.armed_helper", true, vec![armed("std.Nested")]),
            definition("std.unused_helper", true, vec![]),
            definition("std.nested_helper", true, vec![]),
            definition("main", false, vec![Op::Call("std.make".into())]),
        ];
        prune(&mut ops, &[]);
        assert_eq!(
            names(&ops),
            [
                "__plenty_class_std.Armed.__del__",
                "__plenty_class_std.Nested.__del__",
                "std.make",
                "std.armed_helper",
                "std.nested_helper",
                "main"
            ]
        );
    }

    #[test]
    fn follows_indirect_callbacks_and_adapter_calls_inside_control_flow() {
        let signature = Arc::new(CallableSig {
            inputs: vec![],
            output: None,
        });
        let job = Arc::new(crate::executor::Job {
            adapter: "executor_adapter".into(),
            worker: "std.job".into(),
            input: Ty::I64,
            output: Ty::I64,
            closure: None,
            shared: None,
            handle: false,
        });
        let task = Arc::new(crate::threading::Task {
            adapter: "thread_adapter".into(),
            worker: "std.worker".into(),
            inputs: vec![],
            output: Ty::Unit,
            closure: None,
        });
        let callbacks = vec![
            Op::FunctionAddress("std.address".into(), signature.clone()),
            Op::BoxedCallableNew(
                Box::new(Ty::Unit),
                "std.boxed".into(),
                signature,
                crate::op::BoxedMode::Shared,
            ),
            Op::Executor(crate::executor::ExecutorOp::Submit(job, false)),
            Op::Thread(crate::threading::ThreadOp::Start(task, 0, false)),
        ];
        let mut ops = vec![definition(
            "main",
            false,
            vec![Op::Loop {
                condition: Arc::default(),
                body: callbacks.into(),
            }],
        )];
        for name in [
            "std.address",
            "std.boxed",
            "std.job",
            "std.worker",
            "std.dead",
        ] {
            ops.push(definition(name, true, vec![]));
        }
        ops.push(Op::Call("main".into()));
        for executable in [false, true] {
            let mut pruned = ops.clone();
            prune_for_emission(&mut pruned, &[], executable);
            assert_eq!(
                names(&pruned),
                ["main", "std.address", "std.boxed", "std.job", "std.worker"]
            );
        }
    }

    #[test]
    fn real_standard_library_remains_valid_and_deterministic_after_pruning() {
        let mut heap = Heap::default();
        let program = crate::frontend::compile(
            "import std.http.server\n\ndef main() -> Result[(), IoError]:\n    print(42)?\n    Ok(())\n",
            &mut heap,
        ).unwrap();
        crate::op::check(&program.ops).unwrap();
        let before = names(&program.ops).len();
        let mut ops = program.ops;
        prune(&mut ops, &program.exports);
        assert!(names(&ops).len() < before);
        crate::op::check(&ops).unwrap();
        let serial = super::super::object_bytes(
            ops.clone(),
            &heap,
            Some(true),
            &[],
            None,
            1,
            Build::default(),
        )
        .unwrap();
        let parallel =
            super::super::object_bytes(ops, &heap, Some(true), &[], None, 8, Build::default())
                .unwrap();
        assert_eq!(serial, parallel);
    }

    #[test]
    fn executable_roots_keep_callbacks_droppers_and_concrete_generators() {
        let signature = Arc::new(CallableSig {
            inputs: vec![],
            output: None,
        });
        let mut root = definition(
            "main",
            false,
            vec![
                Op::FunctionAddress("callback".into(), signature),
                armed("Resource"),
                Op::Call("generator".into()),
            ],
        );
        let Op::DefineFn(_, function) = &mut root else {
            unreachable!()
        };
        function.locals = vec![crate::generator::ty(
            Ty::I64,
            Some("stored_generator".into()),
        )]
        .into();
        let mut ops = vec![
            root,
            definition("callback", false, vec![]),
            definition(
                &crate::record::method("Resource", "__del__"),
                false,
                vec![Op::Call("drop_helper".into())],
            ),
            definition("drop_helper", false, vec![]),
            definition("generator", false, vec![Op::Call("resume_helper".into())]),
            definition("stored_generator", false, vec![]),
            definition("resume_helper", true, vec![]),
            definition("unused", false, vec![Op::Call("std.optional".into())]),
            definition("std.optional", true, vec![]),
            Op::Call("main".into()),
        ];
        prune_for_emission(&mut ops, &[], true);
        assert_eq!(
            names(&ops),
            [
                "main",
                "callback",
                "__plenty_class_Resource.__del__",
                "drop_helper",
                "generator",
                "stored_generator",
                "resume_helper"
            ]
        );
    }

    fn coff_object(reached: bool, executable: bool, optimize: bool) -> Vec<u8> {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("native.plentyi"),
            "pub extern def missing() -> () = \"pldf_v1_missing\"\n",
        )
        .unwrap();
        let source = temp.path().join("main.plenty");
        std::fs::write(&source, format!(
            "import native\ndef unused() -> ():\n    native.missing()\ndef main() -> ():\n    {}\n",
            if reached { "unused()" } else { "pass" })).unwrap();
        let mut heap = Heap::default();
        let program = Sources::file(&source, None)
            .unwrap()
            .compile(&mut heap, crate::UncheckedMode::Trap)
            .unwrap();
        crate::op::check(&program.ops).unwrap();
        let mut product = super::super::object_module(
            program.ops,
            &heap,
            None,
            &[],
            None,
            2,
            Build {
                prune_user: executable,
                optimize,
                target: Some("x86_64-pc-windows-msvc"),
                ..Default::default()
            },
        )
        .unwrap()
        .finish()
        .unwrap();
        // Expose only the fixture's no-runtime entry, without manufacturing
        // definitions for any import. The bodies use the production pipeline.
        let entry = product.object.symbol_id(b"__plenty_fn_main").unwrap();
        product.object.symbol_mut(entry).scope = object::SymbolScope::Linkage;
        product.emit().unwrap()
    }

    fn coff_link(bytes: &[u8]) -> std::process::Output {
        use object::{Object, ObjectSection, ObjectSymbol};
        let object = object::File::parse(bytes).unwrap();
        let mut runtime = object::write::Object::new(
            object::BinaryFormat::Coff,
            object::Architecture::X86_64,
            object::Endianness::Little,
        );
        let text = runtime.section_id(object::write::StandardSection::Text);
        runtime.set_section_data(text, vec![0xc3], 1);
        // The production pipeline declares the packaged runtime's symbols even
        // when no body refers to them. Supply only those declarations for this
        // non-executed cross-target link; a runtime operation would invalidate
        // this fixture. Never define optional-engine or arbitrary native imports.
        for symbol in object.symbols().filter(|symbol| symbol.is_undefined()) {
            let name = symbol.name().unwrap();
            if !name.starts_with("plenty_") {
                continue;
            }
            assert!(
                !object.sections().any(|section| section
                    .relocations()
                    .any(|(_, relocation)| relocation.target()
                        == object::RelocationTarget::Symbol(symbol.index()))),
                "COFF fixture unexpectedly calls runtime symbol {name}"
            );
            runtime.add_symbol(object::write::Symbol {
                name: name.as_bytes().to_vec(),
                value: 0,
                size: 1,
                kind: object::SymbolKind::Text,
                scope: object::SymbolScope::Linkage,
                weak: false,
                section: object::write::SymbolSection::Section(text),
                flags: object::SymbolFlags::None,
            });
        }
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let sysroot = std::process::Command::new(rustc)
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        assert!(sysroot.status.success());
        let linker = std::path::Path::new(std::str::from_utf8(&sysroot.stdout).unwrap().trim())
            .join("lib/rustlib")
            .join(crate::target::compiler_host())
            .join("bin")
            .join(if cfg!(windows) {
                "rust-lld.exe"
            } else {
                "rust-lld"
            });
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("program.obj");
        std::fs::write(&input, bytes).unwrap();
        let runtime_path = temp.path().join("runtime-declarations.obj");
        std::fs::write(&runtime_path, runtime.write().unwrap()).unwrap();
        std::process::Command::new(linker)
            .args([
                "-flavor",
                "link",
                "/nodefaultlib",
                "/entry:__plenty_fn_main",
                "/subsystem:console",
                "/opt:ref",
            ])
            .arg(format!(
                "/out:{}",
                temp.path().join("program.exe").display()
            ))
            .arg(input)
            .arg(runtime_path)
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "COFF link regression requires the Rust toolchain's bundled rust-lld: {error}"
                )
            })
    }

    #[test]
    fn coff_executable_omits_unreached_imports_but_keeps_reached_link_errors() {
        use object::{Object, ObjectSymbol};
        for optimize in [false, true] {
            let inspectable = coff_object(false, false, optimize);
            let output = coff_link(&inspectable);
            assert!(
                !output.status.success(),
                "retained COFF helper must reproduce the original failure"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("pldf_v1_missing"),
                "stderr: {} stdout: {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
            let bytes = coff_object(false, true, optimize);
            let object = object::File::parse(bytes.as_slice()).unwrap();
            assert!(!object.symbols().any(|symbol| {
                symbol
                    .name()
                    .is_ok_and(|name| name == "pldf_v1_missing" || name == "__plenty_fn_unused")
            }));
            let output = coff_link(&bytes);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = coff_link(&coff_object(true, true, optimize));
            assert!(
                !output.status.success(),
                "reached missing import must remain a linker error"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("pldf_v1_missing"),
                "stderr: {} stdout: {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }

    #[test]
    fn program_cache_separates_executable_pruning_from_inspectable_objects() {
        use object::{Object, ObjectSymbol};
        let temp = tempfile::tempdir().unwrap();
        let directory = crate::cache::CacheMode::Directory(temp.path().join("cache"));
        let output = temp.path().join("program.o");
        let sources = Sources::Text("def unused() -> i64:\n    42\ndef main() -> ():\n    pass\n");
        for (executable, hit) in [
            (false, false),
            (false, false),
            (false, true),
            (true, false),
            (true, false),
            (true, true),
            (false, true),
            (true, true),
        ] {
            let cache = Cache::open(&directory).unwrap();
            super::super::program_object(
                &sources,
                &output,
                Build {
                    prune_user: executable,
                    cache: Some(cache.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(cache.stats().program, Some(hit));
            let bytes = std::fs::read(&output).unwrap();
            let object = object::File::parse(bytes.as_slice()).unwrap();
            let retained = object.symbols().any(|symbol| {
                symbol
                    .name()
                    .is_ok_and(|name| name.ends_with("__plenty_fn_unused"))
            });
            assert_eq!(retained, !executable);
        }
    }

    #[test]
    fn executable_pruning_preserves_running_generator_closure_and_destructor() {
        let source = r#"
class Resource:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def values() -> Generator[i64]:
    yield 40
    yield 2
def unused() -> i64:
    99
def main() -> Result[(), IoError | OverflowError[i64]]:
    owner = Resource(7)
    mut total = 0
    for value in values():
        total += value
    offset = 10
    callback = def [offset](value: i64) -> Result[i64, OverflowError[i64]]:
        Ok(value + offset)
    print(total, callback(1)?)?
    Ok(())
"#;
        let temp = tempfile::tempdir().unwrap();
        let object = temp.path().join("program.o");
        let executable = temp.path().join(if cfg!(windows) {
            "program.exe"
        } else {
            "program"
        });
        super::super::program_object(
            &Sources::Text(source),
            &object,
            Build {
                prune_user: true,
                ..Default::default()
            },
        )
        .unwrap();
        super::super::link_object(
            &temp,
            &object,
            &executable,
            &crate::CompileOptions::default(),
            false,
        )
        .unwrap();
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "42 11\n7\n"
        );
    }
}
