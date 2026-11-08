"""Exercise the book's fragment decoration without a browser dependency."""

import pathlib
import subprocess
import unittest


class FragmentLabelsTests(unittest.TestCase):
    def test_labels_leave_source_and_other_fences_unchanged(self):
        script = pathlib.Path(__file__).resolve().parents[1] / "book/theme/plenty-highlight.js"
        subprocess.run(
            ["node", "-e", r"""
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
function element() {
    return {
        children: [],
        classList: { contains(name) { return name === 'plenty-fragment-label'; } },
        appendChild(child) { this.children.push(child); },
    };
}
function code(classes) {
    const pre = {
        labels: [],
        before(label) { this.labels.push(label); this.previousElementSibling = label; },
    };
    return { classes, pre, textContent: 'source text', closest() { return pre; } };
}
const fragment = code(['language-plenty', 'fragment']);
const runnable = code(['language-plenty']);
const error = code(['language-plenty-error']);
const companion = code(['language-plenty-file']);
const blocks = [fragment, runnable, error, companion];
for (const version of ['10.7.3', '11.0.0']) {
    const context = {
        document: {
            createElement: element,
            querySelectorAll(selector) {
                const classes = selector.slice('code.'.length).split('.');
                return blocks.filter(block => classes.every(c => block.classes.includes(c)));
            },
        },
        hljs: {
            versionString: version,
            registerLanguage() {},
            highlight(...args) { return { value: version.startsWith('10.') ? args[1] : args[0] }; },
        },
    };
    // Repeated execution must not duplicate labels.
    vm.runInNewContext(fs.readFileSync(process.argv[1], 'utf8'), context);
    vm.runInNewContext(fs.readFileSync(process.argv[1], 'utf8'), context);
}
assert.equal(fragment.pre.labels.length, 1);
const label = fragment.pre.labels[0];
assert.equal(label.className, 'plenty-fragment-label');
assert.equal(label.children[0].textContent, 'Fragment · not executed');
for (const block of blocks) {
    assert.equal(block.textContent, 'source text');
    assert.equal(block.innerHTML, 'source text');
}
for (const block of [runnable, error, companion]) assert.equal(block.pre.labels.length, 0);
""", str(script)],
            check=True,
        )


if __name__ == "__main__":
    unittest.main()
