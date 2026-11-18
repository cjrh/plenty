//! Explicit application C callbacks share the library ownership/error adapters.
use super::*;

pub(super) fn compile(
    sources: &Sources,
    output: &Path,
    options: &crate::CompileOptions,
    test_hooks: bool,
    executable: bool,
) -> Result<crate::frontend::Inputs> {
    if options.cpu != crate::CpuLevel::Baseline {
        return Err("executable exports currently require the baseline CPU level".into());
    }
    if test_hooks || options.test_hooks || options.fuzz.is_some() || options.distribution {
        return Err(
            "executable exports exclude test hooks, fuzzing and distribution output".into(),
        );
    }
    let name = options.executable_exports.as_deref().unwrap();
    crate::exports::check_library_name(name)?;
    let mut heap = Heap::default();
    let program = sources.compile(&mut heap)?;
    op::check(&program.ops)?;
    let objects = crate::foreign::exported_objects(&program.ops);
    for symbol in program
        .exports
        .iter()
        .map(|export| &export.symbol)
        .chain(objects.iter().map(|object| &object.symbol))
    {
        if symbol == "main" || symbol.starts_with("plenty_") {
            return Err(format!(
                "executable export `{symbol}` conflicts with the application runtime namespace"
            )
            .into());
        }
    }
    let interface = crate::exports::Interface::new(name, &program.exports, &objects)?;
    interface.validate_imports(&program.imported_symbols)?;
    let directory = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let header_name = format!("{name}.h");
    let header = directory.join(&header_name);
    // Both files have the same parent, which may be on a case-insensitive mount.
    if output.file_name().is_some_and(|file| {
        file.as_encoded_bytes()
            .eq_ignore_ascii_case(header_name.as_bytes())
    }) {
        return Err("application output conflicts with its executable exports header".into());
    }
    let destinations = [output, header.as_path()];
    crate::library::publication::validate(&destinations, &program.source_paths)?;
    let workspace = tempfile::Builder::new()
        .prefix(".plenty-build-")
        .tempdir_in(directory)?;
    let object = workspace.path().join("application.o");
    // Bypass whole-program caching: the header and checked export set must come
    // from this same frontend result. Ordinary builds retain their cache path.
    let build = Build::new(options, false).for_executable();
    let cache = build.cache.clone();
    emit_object(
        program.ops,
        &heap,
        &object,
        Some(program.returns_status),
        &program.exports,
        Some(&interface),
        build,
    )?;
    report_cache(options, cache.as_deref());
    let built = if executable {
        let built = workspace.path().join("application");
        link_object(&workspace, &object, &built, options, false)?;
        built
    } else {
        object
    };
    let staged_header = workspace.path().join("header");
    std::fs::write(&staged_header, interface.header)?;
    crate::library::publication::validate(&destinations, &program.source_paths)?;
    crate::library::publication::publish(
        workspace,
        &[(built, output.to_owned()), (staged_header, header)],
    )?;
    Ok(program.inputs)
}
