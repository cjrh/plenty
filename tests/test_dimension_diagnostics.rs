//! Alias explanations use the diagnostic's lexical scope, never type identity.
mod support;

const BASES: &str = "dimension Length(\"m\")\ndimension Time(\"s\")\n";

fn error(source: &str) -> String {
    plenty::check_source(source).unwrap_err().to_string()
}

#[test]
fn first_declared_alias_is_explained_beside_the_normal_form() {
    let source = format!(
        "{BASES}
dimension Velocity = Later
dimension Later = Length / Time
dimension Acceleration = Velocity / Time
def main() -> ():
    v = Quantity[f64, Later](1.0)
    a = Quantity[f64, Acceleration](1.0)
    bad = v + a
"
    );
    let found = error(&source);
    assert!(found.contains("requires equal dimensions"), "{found}");
    assert!(
        found.contains("dimension alias `Velocity` = `Length * Pow[Time, -1]`"),
        "{found}"
    );
    assert!(
        found.contains("dimension alias `Acceleration` = `Length * Pow[Time, -2]`"),
        "{found}"
    );
    assert!(!found.contains("alias `Later`"), "{found}");
}

#[test]
fn annotations_constants_inference_and_nested_types_explain_aliases() {
    let prefix = format!("{BASES}dimension Velocity = Length / Time\n");
    for body in [
        "def main() -> ():\n    x: Quantity[f64, Velocity] = Quantity[f64, Length](1.0)\n",
        "const x: Quantity[f64, Velocity] = Quantity[f64, Length](1.0)\ndef main() -> ():\n    pass\n",
        "def same[D: Dimension](a: Quantity[f64, D], b: Quantity[f64, D]) -> ():\n    pass\ndef main() -> ():\n    same(Quantity[f64, Velocity](1.0), Quantity[f64, Length](1.0))\n",
        "def main() -> Result[(), AllocError]:\n    xs: list[Quantity[f64, Velocity]] = [Quantity[f64, Length](1.0)]?\n    Ok(())\n",
        "def main() -> ():\n    x: Option[Quantity[f64, Velocity]] = Some(Quantity[f64, Length](1.0))\n",
        "def main() -> ():\n    x = Quantity[f32, Velocity].from(Quantity[f64, Length](1.0))\n",
        "def main() -> ():\n    mut x = Quantity[f64, Velocity](1.0)\n    x += Quantity[f64, Length](1.0)\n",
    ] {
        let found = error(&(prefix.clone() + body));
        assert!(
            found.contains("dimension alias `Velocity` = `Length * Pow[Time, -1]`"),
            "{body}\n{found}"
        );
        assert_eq!(
            found.matches("dimension alias `Velocity`").count(),
            1,
            "{found}"
        );
    }
}

#[test]
fn generic_parameters_shadow_aliases_without_changing_their_identity() {
    let found = error(&format!(
        "{BASES}
dimension D = Length / Time
dimension Speed = D
def bad[D: Dimension](q: Quantity[f64, D]) -> ():
    invalid = q + Quantity[f64, Time](1.0)
def main() -> ():
    bad(Quantity[f64, Speed](1.0))
"
    ));
    assert!(found.contains("dimension alias `Speed`"), "{found}");
    assert!(!found.contains("alias `D`"), "{found}");
}

#[test]
fn imports_use_visible_spellings_and_source_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("units.plenty"), "pub dimension Length(\"m\")\npub dimension Time(\"s\")\ndimension Private = Length / Time\npub dimension Velocity = Length / Time\npub dimension Speed = Velocity\n").unwrap();
    let main = dir.path().join("main.plenty");
    for (imports, alias, absent) in [
        ("import units as u", "u.Velocity", "u.Speed"),
        (
            "from units import Speed as Fast\nimport units as u",
            "Fast",
            "u.Velocity",
        ),
        (
            "import units as u\nfrom units import Speed as Fast",
            "u.Velocity",
            "Fast",
        ),
    ] {
        std::fs::write(
            &main,
            format!("{imports}\ndef main() -> ():\n    x = u.Quantity\n").replace(
                "    x = u.Quantity",
                "    x: Quantity[f64, u.Length / u.Time] = Quantity[f64, u.Length](1.0)",
            ),
        )
        .unwrap();
        let found = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(
            found.contains(&format!(
                "dimension alias `{alias}` = `units.Length * Pow[units.Time, -1]`"
            )),
            "{found}"
        );
        assert!(!found.contains(&format!("alias `{absent}`")), "{found}");
        assert!(!found.contains("Private"), "{found}");
    }
    // Importing only the bases does not put the other module's aliases in scope.
    std::fs::write(&main, "from units import Length, Time\ndef main() -> ():\n    x: Quantity[f64, Length / Time] = Quantity[f64, Length](1.0)\n").unwrap();
    let found = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(!found.contains("dimension alias"), "{found}");
}

#[test]
fn equal_symbols_do_not_make_nominal_dimensions_match() {
    let found = error(
        "dimension Length(\"m\")\ndimension Other(\"m\")\ndimension Distance = Length\ndef main() -> ():\n    x: Quantity[f64, Other] = Quantity[f64, Length](1.0)\n",
    );
    assert!(
        found.contains("expected Quantity[f64, Other], got Quantity[f64, Length]"),
        "{found}"
    );
    assert!(found.contains("alias `Distance` = `Length`"), "{found}");
    assert!(!found.contains("alias `Distance` = `Other`"), "{found}");
}

#[test]
fn alias_names_do_not_leak_between_compilations_or_into_runtime_formatting() {
    for name in ["Velocity", "Speed", "Velocity"] {
        let found = error(&format!(
            "{BASES}dimension {name} = Length / Time\ndef main() -> ():\n    x: Quantity[f64, {name}] = Quantity[f64, Length](1.0)\n"
        ));
        assert!(found.contains(&format!("alias `{name}`")), "{found}");
        assert_eq!(found.matches("dimension alias").count(), 1);
    }
    support::assert_stdout(
        &format!(
            "{BASES}
dimension Velocity = Length / Time
dimension Speed = Velocity
def main() -> Result[(), IoError | AllocError]:
    q = Quantity[f64, Speed](2.0)
    v: Quantity[f64, Velocity] = q
    print(q == v, q)?
    print(f\"{{q!r}}\"?)?
    Ok(())
"
        ),
        "True 2.0 m/s\nQuantity[f64, Length * Pow[Time, -1]](2.0)\n",
    );
}

#[test]
fn dimensionless_aliases_and_invalid_roots_keep_their_normal_forms() {
    let found = error(&format!(
        "{BASES}
dimension Ratio = Length / Length
def main() -> ():
    x: f64 = Quantity[f64, Ratio](1.0)
"
    ));
    assert!(found.contains("alias `Ratio` = `One`"), "{found}");
    let found = error(&format!(
        "{BASES}
dimension Volume = Pow[Length, 3]
def root[D: Dimension](q: Quantity[f64, D]) -> Quantity[f64, Root[D, 2]]:
    Quantity[f64, Root[D, 2]](q.get())
def main() -> ():
    x = root(Quantity[f64, Volume](1.0))
"
    ));
    assert!(found.contains("not a whole dimension"), "{found}");
    assert!(
        found.contains("alias `Volume` = `Pow[Length, 3]`"),
        "{found}"
    );
    let found = error(&format!(
        "{BASES}
dimension Velocity = Length / Time
def invalid(value: Velocity) -> ():
    pass
def main() -> ():
    pass
"
    ));
    assert!(found.contains("has no values"), "{found}");
    assert!(found.contains("alias `Velocity`"), "{found}");
}

#[test]
fn module_body_errors_and_editor_diagnostics_keep_their_own_scope() {
    let dir = tempfile::tempdir().unwrap();
    let units = dir.path().join("units.plenty");
    std::fs::write(&units, "pub dimension Length(\"m\")\npub dimension Time(\"s\")\ndimension LocalSpeed = Length / Time\npub def bad[D: Dimension](q: Quantity[f64, D]) -> ():\n    invalid = q + Quantity[f64, Time](1.0)\n").unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, "import units as u\ndimension CallerSpeed = u.Length / u.Time\ndef main() -> ():\n    u.bad(Quantity[f64, CallerSpeed](1.0))\n").unwrap();
    let found = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(found.contains("alias `LocalSpeed`"), "{found}");
    assert!(!found.contains("alias `CallerSpeed`"), "{found}");
    assert!(found.contains("units.plenty"), "{found}");
    let analysis = plenty::ide::analyze(&main, None, Default::default());
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|d| d.message.contains("alias `LocalSpeed`")),
        "{:?}",
        analysis.diagnostics
    );
    // Replace the imported file for a fresh analysis; no stale scope survives.
    std::fs::write(
        &units,
        std::fs::read_to_string(&units)
            .unwrap()
            .replace("LocalSpeed", "RenamedSpeed"),
    )
    .unwrap();
    let analysis = plenty::ide::analyze(&main, None, Default::default());
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|d| d.message.contains("alias `RenamedSpeed`")),
        "{:?}",
        analysis.diagnostics
    );
    assert!(
        analysis
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("LocalSpeed"))
    );
}

#[test]
fn parallel_source_checks_have_independent_alias_scopes() {
    std::thread::scope(|scope| {
        for n in 0..8 {
            scope.spawn(move || {
                let found = error(&format!("{BASES}dimension Speed{n} = Length / Time\ndef main() -> ():\n    x: Quantity[f64, Speed{n}] = Quantity[f64, Length](1.0)\n"));
                assert!(found.contains(&format!("alias `Speed{n}`")), "{found}");
                assert_eq!(found.matches("dimension alias").count(), 1);
            });
        }
    });
}
