/// The locked backend sources distinguish fork revisions in cache keys.
pub(crate) fn backend_sources(lock: &str) -> String {
    ["cranelift-codegen", "regalloc2"]
        .map(|name| {
            let package_name = format!("name = \"{name}\"");
            let source = lock
                .split("[[package]]")
                .find(|package| package.lines().any(|line| line == package_name))
                .map(|package| {
                    package
                        .lines()
                        .find_map(|line| line.strip_prefix("source = "))
                        .map_or("path", |source| source.trim_matches('"'))
                })
                .unwrap_or("unknown");
            format!("{name} {source}")
        })
        .join("; ")
}
