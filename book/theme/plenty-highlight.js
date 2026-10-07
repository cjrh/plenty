// Highlight Plenty fences with the Python grammar. mdBook marks a fence such as
// ```plenty-file geometry.plenty with the class `language-plenty-file`.
// book.js has already highlighted these blocks as plain text; redo them.
(function () {
    if (typeof hljs === "undefined") return;
    // mdBook bundles highlight.js 10; version 11 changed the arguments.
    // Python rejects `?`, so ignore illegal input or nothing is highlighted.
    const python = (code) =>
        hljs.versionString.startsWith("10.")
            ? hljs.highlight("python", code, true)
            : hljs.highlight(code, { language: "python", ignoreIllegals: true });
    for (const kind of ["plenty", "plenty-error", "plenty-file"]) {
        for (const code of document.querySelectorAll("code.language-" + kind)) {
            code.innerHTML = python(code.textContent).value;
        }
    }
})();
