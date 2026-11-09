//! Emit native data symbols before pruning can remove their Plenty accessors.
use super::*;

pub(super) fn define(
    objects: &[crate::foreign::ExportObject],
    module: &mut ObjectModule,
) -> Result<()> {
    let mut ids = std::collections::BTreeMap::new();
    for object in objects {
        ids.insert(
            object.symbol.as_str(),
            module.declare_data(&object.symbol, Linkage::Export, false, false)?,
        );
    }
    for object in objects {
        let mut data = cranelift_module::DataDescription::new();
        data.define(object.bytes.clone().into_boxed_slice());
        data.set_align(object.align);
        data.set_used(true);
        for relocation in &object.relocations {
            if relocation.function {
                let Some(cranelift_module::FuncOrDataId::Func(id)) =
                    module.declarations().get_name(&relocation.symbol)
                else {
                    return Err(format!("missing C callback export `{}`", relocation.symbol).into());
                };
                let reference = module.declare_func_in_data(id, &mut data);
                data.write_function_addr(relocation.offset, reference);
            } else {
                let reference =
                    module.declare_data_in_data(ids[relocation.symbol.as_str()], &mut data);
                data.write_data_addr(relocation.offset, reference, 0);
            }
        }
        module.define_data(ids[object.symbol.as_str()], &data)?;
    }
    Ok(())
}
