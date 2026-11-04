//! Compile CLIF files with the flags Plenty's release builds use and print
//! the optimized CLIF and the disassembly. See ../README.md.
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use std::sync::Arc;

fn isa() -> Arc<dyn isa::TargetIsa> {
    // As `baseline_isa` in src/codegen.rs, for x86-64 without optional
    // CPU features.
    let mut flags = settings::builder();
    for (name, value) in [
        ("regalloc_algorithm", "backtracking"),
        ("opt_level", "speed"),
        ("is_pic", "true"),
        ("enable_llvm_abi_extensions", "true"),
        ("preserve_frame_pointers", "true"),
        ("enable_probestack", "true"),
        ("probestack_strategy", "inline"),
        ("enable_verifier", "true"),
    ] {
        flags.set(name, value).unwrap();
    }
    isa::lookup("x86_64-unknown-linux-gnu".parse().unwrap())
        .unwrap()
        .finish(settings::Flags::new(flags))
        .unwrap()
}

fn main() {
    let isa = isa();
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path).unwrap();
        let functions = cranelift_reader::parse_functions(&text).unwrap();
        for function in functions {
            let mut context = cranelift_codegen::Context::for_function(function);
            context.set_disasm(true);
            let code = context
                .compile(&*isa, &mut cranelift_control::ControlPlane::default())
                .unwrap_or_else(|error| panic!("{path}: {:?}", error.inner));
            let bytes = code.code_buffer().to_vec();
            println!("==== {path} {}", context.func.name);
            println!("{}", context.func.display());
            let capstone = isa.to_capstone().unwrap();
            for insn in capstone.disasm_all(&bytes, 0).unwrap().iter() {
                println!(
                    "{:6x}:  {} {}",
                    insn.address(),
                    insn.mnemonic().unwrap_or(""),
                    insn.op_str().unwrap_or("")
                );
            }
        }
    }
}
