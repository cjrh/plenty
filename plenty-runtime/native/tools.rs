//! Resolve tools for the Cargo target, never silently use host cc in a cross build.
use std::ffi::OsString;

pub fn tool(kind: &str) -> OsString {
    resolve(
        kind,
        &std::env::var("TARGET").expect("Cargo TARGET"),
        &std::env::var("HOST").expect("Cargo HOST"),
        |key| {
            println!("cargo:rerun-if-env-changed={key}");
            std::env::var_os(key)
        },
    )
}

fn resolve(
    kind: &str,
    target: &str,
    host: &str,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> OsString {
    for key in [
        format!("{kind}_{target}"),
        format!("{kind}_{}", target.replace('-', "_")),
        format!("TARGET_{kind}"),
        kind.to_owned(),
    ] {
        if let Some(value) = lookup(&key) {
            return value;
        }
    }
    if target == host {
        return if kind == "CC" { "cc" } else { "ar" }.into();
    }
    let prefix = match target {
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        _ => panic!("set {kind}_{target} explicitly for this cross target"),
    };
    format!("{prefix}-{}", if kind == "CC" { "gcc" } else { "ar" }).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_override_precedes_generic_and_host_tools() {
        let found = resolve(
            "CC",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            |key| match key {
                "CC_aarch64_unknown_linux_gnu" => Some("cross-gcc".into()),
                "CC" => Some("host-cc".into()),
                _ => None,
            },
        );
        assert_eq!(found, "cross-gcc");
    }
    #[test]
    fn cross_default_never_uses_host_cc_or_ar() {
        for (kind, expected) in [
            ("CC", "aarch64-linux-gnu-gcc"),
            ("AR", "aarch64-linux-gnu-ar"),
        ] {
            assert_eq!(
                resolve(
                    kind,
                    "aarch64-unknown-linux-gnu",
                    "x86_64-unknown-linux-gnu",
                    |_| None
                ),
                expected
            );
        }
        assert_eq!(
            resolve(
                "CC",
                "aarch64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                |_| None
            ),
            "cc"
        );
    }
}
