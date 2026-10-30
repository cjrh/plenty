---
name: consistency-check
description: For maintenance of the Plenty compiler repository. Check one precise question across the whole repository, such as "Do dictionary writes use insert/get_mut instead of indexed assignment?", by fanning out one small read-only haiku agent per area (tutorial chapters, reference subtrees, std module groups, examples, the plenty skill, tests, compiler diagnostics, top-level docs), then verifying their findings. Use for sweeps for stale syntax, renamed APIs or outdated advice. Reports only; it does not fix or file issues unless asked.
---

# Repository consistency check

One question, many areas, one verified report. Small agents read well but
report false positives, so the orchestrator (you) owns the rubric, the area
list and the verification. Nothing is edited during the check.

## 1. Sharpen the question into a rubric

Before launching anything, write a rubric with these parts:

- **Current form**: the recommended spelling, with a short code example.
- **Old forms**: every superseded spelling, with examples.
- **Violation**: what a finding is, in one or two sentences.
- **Exempt**: what may legitimately contain an old form. Always consider:
  - `book/src/proposals/` (including `completed/`) and `experiments/`: history.
    The area script leaves them out unless `--include-history` is given.
  - text that explains, contrasts or rejects the old form, such as a "Python
    habits" table or a reference page saying the old form is a compile error;
  - tests that deliberately use a rejected form to check its diagnostic, and
    phase-guard tests (see `CLAUDE.md`);
  - lookalikes: the same syntax on another type (list versus dictionary
    indexing), Rust code in Rust sources, unrelated identifiers.
- **Search hints**: Python regexes for the old forms (they pre-screen areas;
  see step 2), plus the conceptual shape for cases a regex misses, such as
  "a loop that inserts into a dictionary through a helper". Test each regex
  against one example of every old form before launching; a regex for
  `d[k] = v` misses `d[k].x = v`.
- **Where violations hide**: executable `plenty` fences in the book compile
  in the test suite, so a rejected form there already fails CI. For syntax
  questions, point agents at prose, `plenty fragment` and `text` fences,
  comments, diagnostic text and the skill.

Establish the current form from the reference (`book/src/design/`), the
[cheat sheet](../../../book/src/design/examples/cheatsheet.md) and the
implementation, not from memory; a quick `cargo run --bin plenty -- --check`
probe settles whether an old form is still accepted. If the question is
ambiguous (which old forms, which areas, whether prose mentions count), ask
the user before launching.

## 2. Choose the areas

[`scripts/consistency_areas.py`](../../../scripts/consistency_areas.py)
derives the area map from `git ls-files`, so new files are covered
automatically. Run it from the worktree being checked:

```sh
python3 -B scripts/consistency_areas.py                     # the whole map
python3 -B scripts/consistency_areas.py --pattern 'REGEX' --pattern 'REGEX2' \
    --hits-only --pack --skip runtime --skip engines --skip src-codegen
python3 -B scripts/consistency_areas.py ... --json > "$SCRATCH/areas.json"
```

- `read` areas (book pages, std sources and their reference pages, examples,
  the plenty skill) are sized for an agent to read in full (`--budget`,
  default 400 KB).
- `scan` areas (`tests-*`, `src-*`, `runtime`, `engines`) are large Rust trees;
  their agent searches and reads around each hit.
- `--hits-only` keeps only files with a regex hit; `--max-hits` (default 80)
  splits an area with more hits; `--pack` merges adjacent small areas of one
  family, such as tutorial chapters, to need fewer agents.
- `--only`/`--skip` select areas by id prefix. Drop areas the rubric makes
  irrelevant: Rust-only trees for a Plenty syntax question; `tests-*` when
  old forms there are expected diagnostics tests.
- Without `--hits-only`, an agent receives every file of its area. Use that
  when the conceptual shape matters more than the regex, for prose areas.
- `std-other` collects std modules missing from `STD_THEMES` in the script.
  Add them to a theme in the same change that uses the map.

Search hints are a pre-screen, not the whole check: a `read` area without a
regex hit can still contain the conceptual shape. Keep high-value prose areas
(tutorial, design, skill, top-level) even when they have no hits if the rubric
has a conceptual shape.

## 3. Confirm the plan

Tell the user the rubric summary, the areas and the agent count (one per area
row). Launch up to 5 agents without asking. For more, get the user's go-ahead
first; offer a narrower `--only` run as the alternative.

## 4. Launch read-only haiku agents

Use parallel `Agent` calls, all in one message, each with `model: "haiku"`
and the general-purpose agent type. This needs no extra tooling, and the
structure is a single fan-out followed by verification you do yourself.

If the session has the Workflow tool and the user approved a run of 10 or more
agents, a workflow is better: `agent(prompt, {model: 'haiku', schema})`
validates the output schema, caps concurrency and can resume. Read the
`workflow-authoring` skill first; use `pipeline()` over the areas and return
the findings, then verify them yourself as in step 5. Calling this skill
counts as opting in to a workflow, but the session guideline keeps workflows
under 10 agents unless the user asks for more.

Give each agent this prompt, filling the braces. Paste the file list; for
more than about 100 files, write it to a scratchpad file and give the path.

````text
You are checking one area of the Plenty compiler repository for one
consistency question. Work read-only: do not edit, create or delete files, do
not run builds or tests, and do not use git commands that change state. Use
Read, Grep and read-only shell commands (rg, sed -n) only.

Repository: {worktree absolute path}
Area: {id} ({kind}). {description}

Rubric:
{the rubric from step 1, including exemptions and search hints}

Files (relative to the repository):
{one path per line}

Method:
- For every file, run the search regexes (rg -n) and read at least 15 lines
  around each hit to decide what it is.
- {read areas} Also read each Markdown page and Plenty source in full and
  look for the conceptual shape the regexes miss.
- {scan areas} Plenty programs in Rust tests are string literals; judge the
  Plenty code, not the Rust around it. Skip Rust code that is not Plenty.
- Classify every candidate: "violation", "exempt" (give the exemption), or
  "unsure". Report exempt candidates too, briefly.
- Quote the exact source line. Do not guess line numbers: take them from rg -n.

Before answering, check that the JSON parses by piping it to
python3 -c 'import json,sys; json.load(sys.stdin)'.

Final answer: only a JSON object, no other text:
{"area": "{id}",
 "files_checked": <number>,
 "findings": [{"file": "...", "line": 0, "snippet": "exact line",
               "verdict": "violation|exempt|unsure",
               "reason": "why, naming the rubric rule",
               "replacement": "suggested current form, or empty",
               "confidence": "high|medium|low"}],
 "notes": "anything the rubric did not cover"}
````

## 5. Verify every finding

Haiku agents misread context. Before reporting:

1. Parse each agent's JSON. Repair an obvious bracket slip yourself. If an
   agent returned prose, an unrecoverable object, or
   `files_checked` below its file count, rerun that area once (split it with
   `--max-hits` or `--budget` if it was too large).
2. For each `violation` and `unsure`, read the cited lines yourself
   (`sed -n 'START,ENDp' FILE`) and confirm the snippet exists and the rubric
   applies. Correct wrong line numbers. Drop false positives and duplicates.
   When a finding depends on whether the compiler accepts a form, probe it
   with `plenty --check` on a small program.
3. Spot-check `exempt` findings with low or medium confidence, and any
   exemption reason that does not match the rubric.
4. If there are more than about 40 findings to verify, delegate verification
   per area to agents on the session model (not haiku), giving each the rubric
   and the findings, and asking it to refute each one from the source.
5. Optionally rerun the step 2 regexes and check that every hit in a `read`
   area was classified, so a silent agent miss is visible.

## 6. Report

Lead with the answer: how many verified violations, in how many files.
Then group verified violations by area, each with file:line, the quoted
snippet and the suggested replacement. Then list:

- areas checked, with agent count and files checked per area;
- areas deliberately skipped and why;
- findings rejected in verification, with counts per reason, so the user can
  judge the agents' precision;
- `unsure` cases you could not settle.

Do not edit files or file issues during the check. Offer, in one line each:
filing one local issue for the verified violations (search `issues.db` for
duplicates first, as `CLAUDE.md` describes), or fixing them in a new worktree
and local PR.

## Example rubric

Question: do dictionary writes use the current API?

- Current: `d.insert(k, v)?` adds or replaces; `get_mut` updates in place:
  `match d.get_mut(k): case Some(v): *v += 1`.
- Old forms: `d[k] = v`, `d[k] += n`, `&mut d[k]`, `d[k].field = x`, a
  mutating method call on `d[k]`. All are compile errors since local PR #696.
- Violation: Plenty code, or prose recommending code, that writes through a
  dictionary index.
- Exempt: list or other non-dictionary indexing; reading `d[k]`; text stating
  that the old form is rejected; tests asserting its diagnostic; Rust code.
- Search hints: `[A-Za-z_][\w.]*\[[^\]\n]+\]\s*[-+*/%|&^]?=(?!=)` (index
  writes), `&mut [A-Za-z_][\w.]*\[` (index borrows) and
  `\]\.\w+\s*[-+*/%|&^]?=(?!=)|\]\.\w+\(` (field writes and method calls
  through an index); conceptually, any update of a counter or field stored in
  a `dict`, and prose saying an index borrow works for dictionary values.
