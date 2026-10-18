// highlight.js grammars for the book's Plenty fences.
//
// mdBook marks a fence such as ```plenty-file geometry.plenty with the class
// `language-plenty-file`. This file loads after book.js has highlighted every
// block, so it registers the grammars and highlights those blocks again.
//
// The word lists follow the compiler: `reserved` and `builtin` in
// src/frontend.rs and `prelude_variant` in src/frontend/enums.rs.
// tests/test_book_highlighting.rs fails when they drift apart. Keep each list
// a plain array of double-quoted strings so that test can read it.
(function () {
    // Reserved words, except the reserved literals below.
    const KEYWORDS = [
        "and", "as", "async", "await", "break", "case", "class", "const",
        "continue", "def", "elif", "else", "enum", "export", "extern", "for",
        "from", "if", "import", "in", "lambda", "match", "mut", "not",
        "opaque", "or", "pass", "protocol", "pub", "return", "struct", "trait",
        "try", "type", "while", "with", "yield",
    ];
    // Reserved literals and the prelude's variants.
    const LITERALS = ["True", "False", "None", "Ok", "Err", "Some", "Nothing"];
    // Lower-case built-in type names. Every capitalized name is shown as a type.
    const TYPES = [
        "bool", "bytes", "dict", "f32", "f64", "i8", "i16", "i32", "i64",
        "list", "range", "set", "str", "tuple", "u8", "u16", "u32", "u64",
    ];
    // Lower-case built-in functions from the prelude.
    const BUILT_INS = [
        "append_text", "args", "args_bytes", "channel", "chr", "contains",
        "copy", "decode_utf8", "drop", "encode_utf8", "flush_stderr",
        "flush_stdout", "input", "input_bytes", "len", "next", "open", "ord",
        "print", "read_bytes", "read_text", "select_recv",
        "select_recv_nowait", "select_recv_timeout", "sorted", "spawn",
        "write_bytes", "write_stderr", "write_stderr_bytes", "write_stdout",
        "write_stdout_bytes", "write_text",
    ];

    const IDENT = "[A-Za-z_][A-Za-z0-9_]*";
    const SUFFIX = "(?:[iu](?:8|16|32|64)|f32|f64)";

    function plenty(hljs) {
        const escape = { className: "char-escape", begin: /\\[0nrt\\'"]/ };
        const byteEscape = {
            className: "char-escape",
            begin: /\\(?:x[0-9A-Fa-f]{2}|[0nrt\\'"])/,
        };
        // A `prefix` of `b` makes a byte literal, which adds `\xHH` escapes.
        const string = (quote, multiline, prefix = "") => ({
            className: "string",
            begin: prefix ? new RegExp("\\b" + prefix + quote) : quote,
            end: multiline ? quote : new RegExp(quote + "|$"),
            contains: [prefix ? byteEscape : escape],
        });
        // An f-string: `{{` and `}}` are literal braces, and a field holds
        // ordinary code up to its closing brace.
        const fstring = (quote, multiline) => ({
            className: "string",
            begin: new RegExp("\\bf" + quote),
            end: multiline ? quote : new RegExp(quote + "|$"),
            contains: [
                escape,
                { begin: /\{\{|\}\}/, relevance: 0 },
                { className: "subst", begin: /\{/, end: /\}/, relevance: 0 },
            ],
        });
        // A declaration keyword, then the declared name. `def once(...)`
        // makes a closure that runs once, so there `once` is a keyword.
        const declaration = (keywords, title) => ({
            beginKeywords: keywords,
            end: /[[(:=\n]/,
            returnEnd: true,
            keywords: { keyword: keywords + " once" },
            contains: [{ className: "title", begin: title, relevance: 0 }],
        });
        return {
            name: "Plenty",
            aliases: ["plenty-error", "plenty-file"],
            keywords: {
                keyword: KEYWORDS.join(" "),
                literal: LITERALS.join(" "),
                type: TYPES.join(" "),
                built_in: BUILT_INS.join(" "),
            },
            contains: [
                hljs.HASH_COMMENT_MODE,
                fstring('"""', true),
                fstring("'''", true),
                fstring('"', false),
                fstring("'", false),
                string('"""', true, "b"),
                string("'''", true, "b"),
                string('"', false, "b"),
                string("'", false, "b"),
                string('"""', true),
                string("'''", true),
                string('"', false),
                string("'", false),
                declaration("def", new RegExp("\\b(?!once\\b)" + IDENT)),
                declaration("class enum protocol type", new RegExp(IDENT)),
                {
                    className: "number",
                    begin: new RegExp(
                        "(?:\\b\\d[\\d_]*(?:\\.[\\d_]*)?|\\.\\d[\\d_]*)" +
                            "(?:[eE][+-]?[\\d_]+)?" + SUFFIX + "?\\b"
                    ),
                    relevance: 0,
                },
                // Field and method names after a dot are not built-ins.
                { begin: new RegExp("\\." + IDENT), relevance: 0 },
                {
                    className: "type",
                    begin: new RegExp(
                        "\\b(?!(?:" + LITERALS.join("|") + ")\\b)[A-Z][A-Za-z0-9_]*"
                    ),
                    relevance: 0,
                },
                // Error propagation and borrows.
                { className: "operator", begin: /\?|&(?:mut\b)?/, relevance: 0 },
            ],
        };
    }

    // Compiler diagnostics: a source location and `quoted` Plenty code.
    function diagnostic() {
        return {
            name: "Plenty diagnostic",
            aliases: ["error"],
            contains: [
                { className: "comment", begin: /^[\w./-]+:\d+:\d+:/ },
                {
                    begin: /`/,
                    end: /`/,
                    excludeBegin: true,
                    excludeEnd: true,
                    subLanguage: "plenty",
                },
            ],
        };
    }

    // Program output stays plain text.
    function output() {
        return { name: "Program output", aliases: ["output"], contains: [] };
    }

    function register(hljs) {
        hljs.registerLanguage("plenty", plenty);
        hljs.registerLanguage("plenty-diagnostic", diagnostic);
        hljs.registerLanguage("plenty-output", output);
    }

    if (typeof module !== "undefined" && module.exports) {
        module.exports = register;
        return;
    }
    if (typeof hljs === "undefined") return;
    register(hljs);
    // mdBook bundles highlight.js 10; version 11 changed the arguments.
    const highlight = (language, code) =>
        hljs.versionString.startsWith("10.")
            ? hljs.highlight(language, code, true)
            : hljs.highlight(code, { language, ignoreIllegals: true });
    for (const kind of ["plenty", "plenty-error", "plenty-file", "error", "output"]) {
        for (const code of document.querySelectorAll("code.language-" + kind)) {
            code.innerHTML = highlight(kind, code.textContent).value;
        }
    }
})();
