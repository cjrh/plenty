//! Optional compiler-private ABI observation; pinned to nightly-2026-10-01.
#![feature(rustc_private)]
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::{self, TyCtxt};

struct Inspect;

impl Callbacks for Inspect {
    fn after_analysis<'tcx>(&mut self, _: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        for id in tcx.hir_body_owners() {
            let name = tcx.def_path_str(id.to_def_id());
            if name.starts_with("probe_") {
                let instance = ty::Instance::mono(tcx, id.to_def_id());
                let abi = tcx
                    .fn_abi_of_instance(
                        ty::TypingEnv::fully_monomorphized()
                            .as_query_input((instance, ty::List::empty())),
                    )
                    .unwrap();
                println!(
                    "{name}: convention={:?}; return={:?}",
                    abi.conv, abi.ret.mode
                );
                for (index, arg) in abi.args.iter().enumerate() {
                    println!(
                        "  arg {index}: mode={:?}; size={}; align={}",
                        arg.mode,
                        arg.layout.size.bytes(),
                        arg.layout.align.abi.bytes()
                    );
                }
            }
        }
        Compilation::Stop
    }
}

fn main() {
    rustc_driver::run_compiler(&std::env::args().collect::<Vec<_>>(), &mut Inspect);
}
