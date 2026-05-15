//! The tutorial — written once, here, as data that is both tested and rendered.
//!
//! Each [`Example`] is a real test: its program is run through the interpreter
//! and the resulting stack is checked against the recorded value. The very same
//! examples are rendered into the "Tutorial" section of `README.md`, between
//! the `BEGIN TUTORIAL` / `END TUTORIAL` markers.
//!
//! * `cargo test` — verifies every example, and fails if README.md is stale.
//! * `UPDATE_README=1 cargo test` — regenerates the README tutorial section.
//!
//! Because the rendered output is the *checked* output, the tutorial cannot
//! drift from what the interpreter actually does.

use plenty::Vm;

/// One step of the tutorial: prose, a program, and the stack it leaves.
struct Example {
    /// Heading for the step — becomes a `###` subsection in the README.
    title: &'static str,
    /// Prose shown before the code. May contain Markdown.
    prose: &'static str,
    /// The Plenty program.
    program: &'static str,
    /// The stack the program must leave, as `Vm::stack_repr` renders it.
    stack: &'static str,
}

const EXAMPLES: &[Example] = &[
    Example {
        title: "The stack and numbers",
        prose: "A program is a stream of whitespace-separated words. A number \
                is a word that pushes itself onto the stack.",
        program: "1 2 3",
        stack: "[1 2 3]",
    },
    Example {
        title: "Arithmetic",
        prose: "`+`, `-`, `*` and `/` each pop the top two values and push the \
                result. They read in stack order, so `10 2 -` means `10 - 2`.",
        program: "10 2 -",
        stack: "[8]",
    },
    Example {
        title: "Operators consume only what they need",
        prose: "An operator touches just the top two values; everything below \
                it on the stack is left alone.",
        program: "1 2 3 4 +",
        stack: "[1 2 7]",
    },
    Example {
        title: "Clearing the stack",
        prose: "`:clear` discards every value on the stack.",
        program: "1 2 3 :clear",
        stack: "[]",
    },
    Example {
        title: "Text",
        prose: "A bare word that is not a number or an operator is text. `+` \
                joins two pieces of text instead of adding them.",
        program: "hello world +",
        stack: "[\"helloworld\"]",
    },
    Example {
        title: "Quoted strings",
        prose: "Wrap text in double quotes to push it as a single string. \
                Spaces, operators, and other special characters inside the \
                quotes are taken verbatim.",
        program: r#""hello world" " and goodbye" +"#,
        stack: r#"["hello world and goodbye"]"#,
    },
    Example {
        title: "Functions",
        prose: "Define a function with `: name { signature } \"docstring\" \
                body... ;`. The signature lists inputs as `name Type` pairs, \
                then `->`, then output types; `{ x Int -> Int }` reads as \
                \"takes one `Int` named `x`, leaves one `Int`\". Inside the \
                body, those input names refer to the values passed in — so \
                the body can mention `x` instead of juggling the stack. The \
                docstring describes what the function does. Both the \
                signature and the docstring are mandatory — together they \
                form the function's interface. Call the function by \
                prefixing its name with a colon.",
        program: ": double { x Int -> Int } \"Double an integer.\" x 2 * ;\n\
                  5 :double",
        stack: "[10]",
    },
    Example {
        title: "Functions calling functions",
        prose: "A function body may call other functions. Defining a function \
                never disturbs the stack.",
        program: ": double { x Int -> Int } \"Double an integer.\" x 2 * ;\n\
                  : quad { x Int -> Int } \"Multiply by four.\" x :double :double ;\n\
                  3 :quad",
        stack: "[12]",
    },
    Example {
        title: "Named inputs replace stack juggling",
        prose: "Each input named in the signature is in scope for the whole \
                body — write the name to load it. A function with several \
                inputs can refer to each by name, in any order, as many times \
                as it likes, without `dup`, `swap`, or `rot`.",
        program: ": hypot-sq { a Int b Int -> Int } \
                  \"Square the hypotenuse: a*a + b*b.\" \
                  a a * b b * + ;\n\
                  3 4 :hypot-sq",
        stack: "[25]",
    },
];

const BEGIN_MARKER: &str = "<!-- BEGIN TUTORIAL";
const END_MARKER: &str = "<!-- END TUTORIAL -->";

/// Run every example, check its stack, and render the Markdown that belongs
/// between the tutorial markers. The check and the render share one pass, so
/// the rendered output is always the verified output.
fn verify_and_render() -> String {
    let mut out = String::from("\n");
    for ex in EXAMPLES {
        let mut vm = Vm::new();
        vm.run(ex.program)
            .unwrap_or_else(|e| panic!("tutorial example {:?} failed to run: {e}", ex.title));
        let actual = vm.stack_repr();
        assert_eq!(
            actual, ex.stack,
            "\ntutorial example {:?} is out of date: the interpreter now \
             leaves {actual}, but tests/tutorial.rs records {}.\n",
            ex.title, ex.stack,
        );
        out.push_str(&format!(
            "### {}\n\n{}\n\n```forth\n{}\n```\n\n```\n{}\n```\n\n",
            ex.title, ex.prose, ex.program, ex.stack,
        ));
    }
    out
}

/// Replace the text between the tutorial markers in `readme` with `generated`,
/// leaving the markers themselves and all hand-written prose in place.
fn splice_tutorial(readme: &str, generated: &str) -> String {
    let begin = readme
        .find(BEGIN_MARKER)
        .unwrap_or_else(|| panic!("README.md is missing a line containing `{BEGIN_MARKER}`"));
    let after_begin_line = readme[begin..]
        .find('\n')
        .map(|nl| begin + nl + 1)
        .expect("README.md BEGIN TUTORIAL marker must be on its own line");
    let end = readme
        .find(END_MARKER)
        .unwrap_or_else(|| panic!("README.md is missing a line containing `{END_MARKER}`"));
    assert!(
        end >= after_begin_line,
        "README.md TUTORIAL markers are in the wrong order",
    );
    format!("{}{}{}", &readme[..after_begin_line], generated, &readme[end..])
}

/// Verifies every tutorial example and keeps the README tutorial section in
/// sync. Set `UPDATE_README=1` to rewrite the section instead of checking it.
#[test]
fn readme_tutorial_stays_in_sync() {
    let generated = verify_and_render();
    let current =
        std::fs::read_to_string("README.md").expect("README.md should exist at the package root");
    let updated = splice_tutorial(&current, &generated);

    if std::env::var_os("UPDATE_README").is_some() {
        if updated != current {
            std::fs::write("README.md", &updated).expect("failed to write README.md");
            eprintln!("README.md tutorial section regenerated.");
        }
    } else {
        assert_eq!(
            current, updated,
            "\nREADME.md tutorial section is out of date — \
             run `UPDATE_README=1 cargo test` to regenerate it.\n",
        );
    }
}
