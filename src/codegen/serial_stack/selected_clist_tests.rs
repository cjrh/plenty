use super::selected_bytes_tests::{ADAPTER, compiled_budget};

/// Model both collections in the same executable so one selected owner kind
/// cannot accidentally poison the other kind's budget.
fn clist_adapter(selected: bool, registered: bool) -> String {
    ADAPTER
        .replace(
            "pub class Sink:",
            "pub class Row:\n    pub n: i64\n\npub class Sink:",
        )
        .replace("binary.Bytes", "clist[Row]")
        .replace(
            "PROVIDER",
            if registered {
                "out = alloc.allocator(&root, Forward(root.share())).unwrap()"
            } else {
                "out = root.share()"
            },
        )
        .replace("    mut sink", "    rows = clist[Row]()\n    mut sink")
        .replace(
            "BUFFER",
            if selected {
                "out.slice_clist(&rows)"
            } else {
                "clist[Row].with_capacity(1)"
            },
        )
}

#[test]
fn selected_clist_growth_and_release_require_both_emitted_facts() {
    for selected in [false, true] {
        for registered in [false, true] {
            let source = clist_adapter(selected, registered);
            for (checked, callback) in [
                ("self.data.reserve(1).unwrap()", true),
                ("self.data.append(Row(1)).unwrap()", true),
                ("self.saved = Nothing", true),
                ("drop(self.data.len())", false),
                ("drop(self.data[0].n)", false),
                ("self.data[0].n = 3", false),
            ] {
                let program = source
                    .replace("CHECKED_BODY", checked)
                    .replace("REJECT_BODY", "pass");
                assert_eq!(
                    compiled_budget(&program).unwrap() == u64::MAX,
                    callback && selected && registered,
                    "selected={selected}, registered={registered}, checked={checked}"
                );
            }
            // The rejection path only creates ordinary storage. In a mixed
            // program, though, dropping the Result may release a selected clist.
            let reject = source
                .replace("CHECKED_BODY", "drop(self.data.len())")
                .replace("REJECT_BODY", "drop(clist[Row].with_capacity(0))");
            let budget = compiled_budget(&reject);
            if selected && registered {
                let err = budget.unwrap_err().to_string();
                assert!(
                    err.contains("cannot safely report serialization stack exhaustion"),
                    "{err}"
                );
            } else {
                assert_ne!(budget.unwrap(), u64::MAX);
            }
        }
    }
}

#[test]
fn selected_bytes_do_not_make_clist_growth_unbounded_and_vice_versa() {
    let columns = clist_adapter(false, true)
        .replace(
            "    rows =",
            "    drop(out.slice_bytes(b\"x\"))\n    rows =",
        )
        .replace("CHECKED_BODY", "self.data.reserve(1).unwrap()")
        .replace("REJECT_BODY", "pass");
    assert_ne!(compiled_budget(&columns).unwrap(), u64::MAX);
    let bytes = ADAPTER
        .replace("pub class Sink:", "pub class Row:\n    pub n: i64\n\npub class Sink:")
        .replace("PROVIDER", "out = alloc.allocator(&root, Forward(root.share())).unwrap()\n    rows = clist[Row]()\n    drop(out.slice_clist(&rows))")
        .replace("BUFFER", "binary.from_bytes(b\"x\")")
        .replace("CHECKED_BODY", "self.data.push(1u8).unwrap()")
        .replace("REJECT_BODY", "pass");
    assert_ne!(compiled_budget(&bytes).unwrap(), u64::MAX);
}
