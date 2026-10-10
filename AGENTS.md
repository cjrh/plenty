# Worktrees and pull requests

- Deliver repository changes through GitHub pull requests in `cjrh/plenty`. Use a dedicated branch and worktree for each independent task/worker; reuse an assigned task worktree and PR when continuing that task. Do not implement changes in the main worktree or on the default branch.
- Inspect local changes and existing worktrees before starting. Fetch the remote and branch from its current default branch unless the task explicitly depends on another branch. Preserve other workers' and the user's changes; never reset, stash, or commit them as part of your task.
- Agree on responsibility for files/components before concurrent work. Keep independent tasks in separate PRs; identify dependencies between PRs instead of silently copying unmerged work. Worktree isolation does not prevent conflicting designs or overlapping edits.
- A request to make repository changes includes committing the task's changes, pushing its branch, and opening/updating its PR, unless the user requests local-only work. Use `gh` for PR operations. Open a draft while work or validation remains; mark it ready when the scoped work and required checks are complete.
- The user reviews and merges PRs. Do not merge, enable auto-merge, push directly to the default branch, or rewrite a published branch unless explicitly instructed. Leave the branch and worktree available for review and follow-up.
- Include relevant implementation, tests, reference/tutorial changes, and minimal backlog edits in the same PR. State the problem, resulting behavior, validation actually performed, and remaining limitations; distinguish SQLite issue numbers from GitHub issue numbers. Return the PR link when handing off.
- **Shared issues register exception:** every worktree must read and update `issues.db` in the main (primary) worktree, using its absolute path. Never use a task worktree's tracked copy or replace it with a symlink. Use SQLite transactions and a busy timeout; see `CLAUDE.md` for path discovery and commands. Shared issue metadata is updated directly, but an issue fixed by a PR stays open until that PR is merged. Do not stage stale worktree copies of `issues.db` in feature PRs.

# Project conventions

- Docs are an mdBook in `book/src/`. `SUMMARY.md` lists every page in reading order.
- Track future work, priorities, and task status only in `book/src/backlog.md`. Root `BACKLOG.md` is a link to that page. Reference pages describe implemented behavior and limits; proposals record rationale and alternatives, not competing work queues.
- Always aim to keep the reference (`book/src/design/`) up to date with the latest design decisions and architectural changes.
- ...and likewise, always aim to keep the implementation up to date with the latest design decisions in the reference.
- Keep the tutorial (`book/src/tutorial/`) up to date with learner-visible language changes. Teach implemented behavior with runnable examples; `tests/test_tutorial.rs` checks the guide's examples and expected diagnostics directly.
- Keep pages small. Add a new page to `SUMMARY.md`; `tests/test_tutorial.rs` fails if a page is missing from it.
- A `plenty-file` companion module must be on the same page as the example that uses it.
- Record defects and gaps in implemented behavior (crashes, misleading diagnostics, missing operations, performance concerns) in the SQLite issues register `issues.db`, not in the backlog. Search it (`issues_fts`, FTS5) before adding, and include a reproduction and a suggested fix in the markdown `details`. Set `severity` to `low`, `normal`, or `high`, and work through open issues highest severity first. When an issue is scheduled, cite `issue #N` in the backlog entry; to close one, set `status` to `closed` and fill `resolution` with the fixing commit or the reason it will not be fixed. Never delete issues. See `CLAUDE.md` for schema and commands.
