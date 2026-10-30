# The Plenty agent skill

The `plenty` skill teaches coding agents (Claude Code, Codex, Cursor and
others that read the Agent Skills format) to write Plenty. It is for people
writing Plenty programs; the
[repository search skill](semantic-search-corpus.md) is for work on this
repository. This page covers where the skill lives, how it is published, and
how to trial it.

## Layout

| Path | Contents |
|---|---|
| `.agents/skills/plenty/` | The skill: `SKILL.md` and hand-written `references/` |
| `.claude/skills/plenty` | Symlink to `.agents/skills/plenty`, so Claude Code loads it here |
| `scripts/agent_skill_repo/` | Files that exist only in the published repository: plugin manifests and README |
| `scripts/sync_agent_skill.py` | Assembles the published repository |
| `scripts/skill_trial.py` | Runs fresh agents against the published skill |

Agents working in this checkout use the skill directly. In the checkout, its
links point at book pages such as the
[syntax cheat sheet](../design/examples/cheatsheet.md) and the
[signature index](../design/stdlib/signatures.md), so it never holds a copy
that could fall behind the book. Copies are made only when publishing.

`tests/test_tutorial.rs` runs every executable fence in the skill with the
same fence kinds as the book, so a language change that breaks a skill example
fails the suite. `scripts/test_sync_agent_skill.py` checks that the skill's
links resolve and that the published tree is self-contained.

When a skill passage works around a local issue, an HTML comment names it, for
example `<!-- issue #575 -->`. Remove the workaround when the issue is closed.
The published copy drops these comments.

## Publishing

The published repository, `plentylang/skills`, has one tag per Plenty release,
`vX.Y.Z`, matching the compiler. To assemble it from a release checkout:

```sh
just skill-sync ../skills          # or: python3 -B scripts/sync_agent_skill.py ../skills
```

The script refuses to replace a directory that is not a skills tree, keeps
`.git` in a clone, and replaces everything else. It:

- copies `.agents/skills/plenty/` to `skills/plenty/`, naming the compiler
  version from `Cargo.toml` (`--version` overrides it);
- copies the cheat sheet, the packages page, every standard library page, and
  the builtins and I/O pages into `skills/plenty/references/`;
- rewrites links between copied pages, and reduces links to other pages to
  their text, until the book has a published URL;
- drops issue notes and contributor-only passages;
- adds the README and the `.claude-plugin/` marketplace and plugin manifests,
  with the version filled in.

Commit the result in the published repository and tag it `vX.Y.Z`.

### Contributor-only passages

Some book passages address compiler maintainers, such as how to update a
vendored crate or regenerate the signature index. Enclose them in markers so
the published skill omits them:

```text
<!-- contributors-only -->
To update one, replace its directory with the new release, run
`cargo update -p CRATE --precise VERSION`, and rerun the tests.
<!-- /contributors-only -->
```

Each marker is a line of its own. The markers are invisible in the book.
Unbalanced markers stop the sync.

## Trial runs

A trial gives fresh agents only the published skill and the compiler, asks
them to write three small programs, and checks the results:

```sh
just skill-trial                           # all tasks, the agent's default model
just skill-trial wordfreq --model sonnet   # one task, a chosen model
```

The recipe builds the release compiler first. Trials need the `claude` CLI,
signed in, and spend API credit (at most `--max-budget-usd` per task, default
3). They are not run in CI. `python3 -B scripts/skill_trial.py --help` lists
every option.

| Task | Asks for | Checks |
|---|---|---|
| `wordfreq` | A CLI counting words in a file, with error and usage exit statuses | Output, exit status 1 naming a missing file, exit status 2 for usage |
| `inventory` | Classes, mutation through `&mut self`, and an error enum | Exact output |
| `packages` | A library with tests and an executable depending on it, using `std.json` | `plenty test` passes; exact output of `plenty run` |

Each task runs `claude -p` in a new directory under the system temporary
directory, never inside a checkout, because Claude Code reads `CLAUDE.md` from
parent directories. The agent gets:

- the published tree as a plugin (`--plugin-dir`), as `/plugin install` would
  provide it;
- `plenty` on `PATH`, as a symlink to `target/release/plenty`;
- no user or project settings, `CLAUDE.md`, MCP servers or session history;
- the file tools, `Bash` and `Skill`, without permission prompts.

Because the agent can run shell commands, it could still read files elsewhere.
The prompt tells it not to, and the report lists any tool call that mentions
this repository.

### Reading the results

Each run writes `target/skill-trial/RUN/`:

| File | Contents |
|---|---|
| `report.md` | Summary table, then one section per task |
| `report.json` | The same data, for comparing runs |
| `tasks/NAME/prompt.md` | The prompt sent |
| `tasks/NAME/transcript.jsonl` | The agent's full stream-json transcript |
| `tasks/NAME/files/` | The sources the agent left |
| `skill/` | The published tree the agents used |

The summary table records:

- **Result**: whether every check passed.
- **Skill used**: whether the agent loaded the skill or read its files. A `no`
  means the skill's description did not trigger.
- **`plenty` runs**: compiler invocations the agent made.
- **Failed runs**: invocations that exited nonzero, including command-line
  misuse.
- **Diagnostics**: each compiler error the agent hit.
- **Leaks**: tool calls that reached outside the task and the skill.

Each task's section lists failed checks with expected and actual output, every
diagnostic, and the failed commands with their last lines of output.

To improve the skill, read the diagnostics and failed commands first. Each one
the skill could have prevented points at a missing rule, a misleading example,
or a diagnostic worth improving. Then search the transcript for guesses: places
where the agent searched the references and did not find an answer. Results
vary between runs and models, so compare several runs before changing the skill
for a single failure.
