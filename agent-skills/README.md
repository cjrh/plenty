# Plenty skills for coding agents

This repository contains the `plenty` skill: instructions and reference pages
that help coding agents (Claude Code, Codex, Cursor, and others that read the
[Agent Skills](https://agentskills.io) format) write correct
[Plenty](https://github.com/cjrh/plenty) programs.

This release matches **Plenty @VERSION@**. Each Plenty release has a matching
tag here, `v@VERSION@`; install the tag that matches `plenty --version`.

The reference pages under `skills/plenty/references/` (other than
`workflow.md`, `errors.md`, `ownership.md`, and `diagnostics.md`) are generated
from the Plenty book for that release. Report problems against Plenty, not by
editing the generated pages.

## Install

### Claude Code plugin

```text
/plugin marketplace add plentylang/skills
/plugin install plenty@plentylang
```

To pin a release, add the marketplace at its tag. The plugin lives in the
same repository, so the tag pins the skill too:

```text
/plugin marketplace add plentylang/skills#v@VERSION@
```

Inside a plugin the skill is named `plenty:plenty`. Agents load it
automatically for Plenty work; you can also invoke it as `/plenty:plenty`.

### Plain skill directory

Any agent that discovers skills from a directory can use a clone. Clone the
release tag and link or copy `skills/plenty`:

```sh
git clone --depth 1 --branch v@VERSION@ https://github.com/plentylang/skills.git plenty-skills

# Claude Code, for every project
mkdir -p ~/.claude/skills && cp -R plenty-skills/skills/plenty ~/.claude/skills/

# Claude Code, one project
mkdir -p .claude/skills && cp -R plenty-skills/skills/plenty .claude/skills/

# One project, for agents that read .agents/skills (Codex and others)
mkdir -p .agents/skills && cp -R plenty-skills/skills/plenty .agents/skills/
```

For a project that should pin the skill version with its code, commit the
copied `skills/plenty` directory, or add this repository as a Git submodule.

When you upgrade Plenty, install the skill tag for the new version.

## License

MIT OR Apache-2.0, the same as Plenty.
