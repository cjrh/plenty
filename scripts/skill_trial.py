#!/usr/bin/env python3
"""Trial the published `plenty` agent skill with a fresh coding agent.

Each task runs `claude -p` in an empty directory outside this repository, with
the synced skill (scripts/sync_agent_skill.py) loaded as a plugin and the
compiler on PATH as its only Plenty context: no user or project settings,
CLAUDE.md, MCP servers or session history. The script then checks the task's
result itself and writes `report.md`, `report.json`, each transcript and the
final sources under `target/skill-trial/RUN/`. It spends API credit and is not
run in CI. See book/src/development/agent-skill.md.
"""

import argparse
from dataclasses import dataclass, field
import datetime
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

import sync_agent_skill

REPO = sync_agent_skill.REPO

PREAMBLE = """\
You are writing Plenty, a programming language. The `plenty` skill describes
the language and its toolchain; use it. The `plenty` compiler is on PATH.
Work only in the current directory. Do not look for Plenty sources, docs or
examples elsewhere on this machine. When finished, reply with a short summary.

Task:
"""


@dataclass
class Check:
    """One command run in the task directory after the agent finishes."""

    name: str
    argv: list
    cwd: str = "."
    exit: int = 0
    stdout: str | None = None
    stdout_contains: str | None = None
    stderr_contains: str | None = None


@dataclass
class Task:
    name: str
    prompt: str
    files: dict = field(default_factory=dict)
    checks: list = field(default_factory=list)


TASKS = [
    Task(
        name="wordfreq",
        prompt="""\
Write `wordfreq.plenty`, a command-line program. `wordfreq FILE` reads FILE,
lowercases it, splits it on whitespace, counts each word, and prints the three
most frequent words as `WORD COUNT` lines, most frequent first, ties in
alphabetical order. With a missing or unreadable FILE it prints an error naming
the file to standard error and exits with status 1. With the wrong number of
arguments it prints a usage line to standard error and exits with status 2.
`sample.txt` is a test input.
""",
        files={"sample.txt": "the cat and the dog\nthe cat and a bird\nThe end\n"},
        checks=[
            Check("compile", ["plenty", "--compile", "wordfreq.plenty", "-o", "wordfreq"]),
            Check("counts", ["./wordfreq", "sample.txt"], stdout="the 4\nand 2\ncat 2\n"),
            Check("missing file", ["./wordfreq", "missing.txt"], exit=1, stderr_contains="missing.txt"),
            Check("usage", ["./wordfreq"], exit=2),
        ],
    ),
    Task(
        name="inventory",
        prompt="""\
Write `inventory.plenty`. Define a class `Item` (name, quantity), a class
`Inventory` holding a list of items, and an error enum for removals.
`Inventory` has `add(name, quantity)` (merging into an existing item),
`remove(name, quantity)` returning a `Result` that fails when the item is
missing or has too few, and `total()`. `main` must:

1. add apple 5, pear 3, apple 2, then print `total: 10`;
2. remove 4 apples and print `apples: 3`;
3. try to remove 1 plum and print `error: no plum`;
4. try to remove 10 pears and print `error: only 3 pear`;
5. exit with status 0.

Print exactly those four lines.
""",
        checks=[
            Check(
                "output",
                ["plenty", "inventory.plenty"],
                stdout="total: 10\napples: 3\nerror: no plum\nerror: only 3 pear\n",
            ),
        ],
    ),
    Task(
        name="packages",
        prompt="""\
Create two Plenty packages in the current directory.

1. `textutil`, a library exporting `slugify(text: &str)`, which lowercases the
   text and replaces each space with `-`. Give it at least two tests that pass
   with `plenty test`.
2. `blog`, an executable depending on `textutil` by path. It prints the slug of
   `Hello Plenty World`, then a JSON object made with `std.json`, on one line:
   `{"title":"Hello Plenty World","slug":"hello-plenty-world"}`.
""",
        checks=[
            Check("library tests", ["plenty", "test"], cwd="textutil", stdout_contains="0 failed"),
            Check(
                "blog output",
                ["plenty", "run"],
                cwd="blog",
                stdout='hello-plenty-world\n{"title":"Hello Plenty World","slug":"hello-plenty-world"}\n',
            ),
        ],
    ),
]

# Strings that mean the agent looked past its intended context.
LEAK_PATTERNS = [str(REPO), "plenty-worktrees", "book/src", "/.claude/", "issues.db"]
SOURCE_SUFFIXES = {".plenty", ".toml", ".lock", ".txt"}


def outside_repository(path):
    """True when neither `path` nor a parent holds project agent instructions.

    Claude Code reads CLAUDE.md files from parent directories, so a task inside
    this checkout would see the contributor instructions. The home directory's
    `.claude` holds user settings, which `--setting-sources project` skips.
    """
    path = path.resolve()
    if path.is_relative_to(REPO):
        return False
    for parent in [path, *path.parents]:
        names = ("CLAUDE.md", "AGENTS.md") if parent == Path.home() else ("CLAUDE.md", "AGENTS.md", ".claude")
        if any((parent / name).exists() for name in names):
            return False
    return True


def default_workdir_root():
    # TMPDIR often points into target/ of a worktree; the agent must not run there.
    root = Path(tempfile.gettempdir())
    return str(root) if outside_repository(root) else "/tmp"


def agent_command(arguments, plugin_dir, prompt):
    command = [
        arguments.agent,
        "-p",
        prompt,
        "--output-format",
        "stream-json",
        "--verbose",
        "--setting-sources",
        "project",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--plugin-dir",
        str(plugin_dir),
        "--tools",
        "Bash,Read,Write,Edit,Glob,Grep,Skill",
        "--allowedTools",
        "Bash Read Write Edit Glob Grep Skill",
        "--permission-mode",
        "dontAsk",
        "--max-budget-usd",
        str(arguments.max_budget_usd),
    ]
    if arguments.model:
        command += ["--model", arguments.model]
    if arguments.effort:
        command += ["--effort", arguments.effort]
    return command


def tool_calls(transcript):
    """Pair each tool call in a stream-json transcript with its result text."""
    calls = {}
    order = []
    result = None
    final = ""
    for line in transcript.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = event.get("type")
        if kind == "result":
            result = event
            final = event.get("result") or final
            continue
        message = event.get("message") or {}
        for block in message.get("content") or []:
            if not isinstance(block, dict):
                continue
            if kind == "assistant" and block.get("type") == "tool_use":
                calls[block["id"]] = {
                    "tool": block["name"], "input": block.get("input", {}), "output": "", "failed": False
                }
                order.append(block["id"])
            elif kind == "user" and block.get("type") == "tool_result":
                content = block.get("content")
                if isinstance(content, list):
                    content = "\n".join(part.get("text", "") for part in content if isinstance(part, dict))
                if block.get("tool_use_id") in calls:
                    calls[block["tool_use_id"]]["output"] = content or ""
                    calls[block["tool_use_id"]]["failed"] = bool(block.get("is_error"))
    return [calls[key] for key in order], result, final


# A compiler invocation, as opposed to a command that only mentions a path.
COMPILER = re.compile(r"(?:^|[\s;&|(])plenty\s+(?:--check|--compile|check|run|test|build|new|add|init|\S+\.plenty)")
DIAGNOSTIC = re.compile(r"^error: (?:\S*/)?(\S+\.plenty:\d+:\d+: .*)$", re.MULTILINE)


def compiler_calls(calls):
    return [
        call for call in calls
        if call["tool"] == "Bash" and COMPILER.search(call["input"].get("command", ""))
    ]


def diagnostics(calls):
    """Compiler diagnostics the agent hit, with the command that produced each."""
    return [
        {"command": call["input"]["command"], "error": match[1]}
        for call in compiler_calls(calls)
        for match in DIAGNOSTIC.finditer(call["output"])
    ]


def failed_commands(calls):
    """Compiler invocations that exited nonzero, with their output's last lines."""
    return [
        {"command": call["input"]["command"], "output": "\n".join(call["output"].splitlines()[-8:])}
        for call in compiler_calls(calls)
        if call["failed"]
    ]


def leaks(calls):
    found = []
    for call in calls:
        text = json.dumps(call["input"])
        for pattern in LEAK_PATTERNS:
            if pattern in text and "skill-trial-skill" not in text:
                found.append(f"{call['tool']}: {text[:200]}")
                break
    return found


def run_check(check, workdir, env):
    try:
        done = subprocess.run(
            check.argv, cwd=workdir / check.cwd, env=env, capture_output=True, text=True, timeout=300
        )
        code, out, err = done.returncode, done.stdout, done.stderr
    except (OSError, subprocess.TimeoutExpired) as error:
        code, out, err = None, "", str(error)
    passed = code == check.exit
    if check.stdout is not None:
        passed = passed and out == check.stdout
    if check.stdout_contains is not None:
        passed = passed and check.stdout_contains in out
    if check.stderr_contains is not None:
        passed = passed and check.stderr_contains in err
    return {
        "name": check.name,
        "command": " ".join(check.argv),
        "cwd": check.cwd,
        "passed": passed,
        "exit": code,
        "expected_exit": check.exit,
        "expected_stdout": check.stdout,
        "stdout": out,
        "stderr": err,
    }


def copy_sources(workdir, destination):
    for path in sorted(workdir.rglob("*")):
        relative = path.relative_to(workdir)
        if relative.parts[0] == "bin" or "target" in relative.parts or not path.is_file():
            continue
        if path.suffix in SOURCE_SUFFIXES:
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)


def run_task(task, arguments, plugin_dir, run_dir, compiler):
    workdir = Path(tempfile.mkdtemp(prefix=f"plenty-skill-trial-{task.name}-", dir=arguments.workdir_root))
    if not outside_repository(workdir):
        raise SystemExit(f"{workdir} is inside a directory with agent instructions; pass --workdir-root")
    (workdir / "bin").mkdir()
    (workdir / "bin/plenty").symlink_to(compiler)
    for name, text in task.files.items():
        (workdir / name).write_text(text)
    env = dict(os.environ, PATH=f"{workdir / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}")
    env.pop("PLENTY_CACHE", None)
    task_dir = run_dir / "tasks" / task.name
    task_dir.mkdir(parents=True)
    prompt = PREAMBLE + task.prompt
    (task_dir / "prompt.md").write_text(prompt)
    print(f"[{task.name}] running agent in {workdir}", file=sys.stderr)
    started = datetime.datetime.now(datetime.UTC)
    with open(task_dir / "transcript.jsonl", "w") as transcript, open(task_dir / "agent-stderr.txt", "w") as stderr:
        agent = subprocess.run(
            agent_command(arguments, plugin_dir, prompt),
            cwd=workdir, env=env, stdout=transcript, stderr=stderr, timeout=arguments.timeout,
        )
    elapsed = (datetime.datetime.now(datetime.UTC) - started).total_seconds()
    calls, result, final = tool_calls((task_dir / "transcript.jsonl").read_text())
    checks = [run_check(check, workdir, env) for check in task.checks]
    copy_sources(workdir, task_dir / "files")
    if not arguments.keep:
        shutil.rmtree(workdir)
    plenty_commands = [call["input"]["command"] for call in compiler_calls(calls)]
    return {
        "task": task.name,
        "passed": all(check["passed"] for check in checks),
        "agent_exit": agent.returncode,
        "seconds": round(elapsed),
        "turns": (result or {}).get("num_turns"),
        "cost_usd": (result or {}).get("total_cost_usd"),
        "skill_used": any(
            call["tool"] == "Skill" or "skills/plenty" in json.dumps(call["input"]) for call in calls
        ),
        "tool_calls": len(calls),
        "plenty_commands": plenty_commands,
        "diagnostics": diagnostics(calls),
        "failed_commands": failed_commands(calls),
        "leaks": leaks(calls),
        "checks": checks,
        "final_message": final,
        "workdir": str(workdir) if arguments.keep else None,
    }


def fence(text):
    return "```text\n" + (text.rstrip() or "(empty)") + "\n```\n"


def report(results, run_dir, meta):
    lines = [
        "# Plenty skill trial\n",
        f"Run {meta['run']}: compiler {meta['compiler_version']}, model {meta['model'] or 'default'}, "
        f"skill at `{meta['skill']}`.\n",
        "| Task | Result | Checks | Skill used | `plenty` runs | Failed runs | Diagnostics | Turns | Cost (USD) | Leaks |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for result in results:
        passed = sum(check["passed"] for check in result["checks"])
        cost = f"{result['cost_usd']:.2f}" if result["cost_usd"] is not None else "?"
        lines.append(
            f"| [{result['task']}](#{result['task']}) | {'pass' if result['passed'] else 'FAIL'} | "
            f"{passed}/{len(result['checks'])} | {'yes' if result['skill_used'] else 'no'} | "
            f"{len(result['plenty_commands'])} | {len(result['failed_commands'])} | {len(result['diagnostics'])} | {result['turns']} | {cost} | "
            f"{len(result['leaks'])} |"
        )
    lines.append("")
    for result in results:
        name = result["task"]
        lines.append(f"## {name}\n")
        lines.append(
            f"Prompt: [tasks/{name}/prompt.md](tasks/{name}/prompt.md). Transcript: "
            f"[tasks/{name}/transcript.jsonl](tasks/{name}/transcript.jsonl). Final sources: "
            f"`tasks/{name}/files/`. Agent exit status {result['agent_exit']}, {result['seconds']} s, "
            f"{result['tool_calls']} tool calls.\n"
        )
        lines.append("### Checks\n")
        for check in result["checks"]:
            lines.append(f"- {'pass' if check['passed'] else 'FAIL'}: {check['name']} (`{check['command']}` in `{check['cwd']}`)")
            if not check["passed"]:
                lines.append(f"\n  Exit {check['exit']}, expected {check['expected_exit']}.\n")
                if check["expected_stdout"] is not None:
                    lines.append("  Expected stdout:\n\n" + fence(check["expected_stdout"]))
                lines.append("  Stdout:\n\n" + fence(check["stdout"]))
                lines.append("  Stderr:\n\n" + fence(check["stderr"][-2000:]))
        lines.append("\n### Diagnostics hit\n")
        if result["diagnostics"]:
            lines.extend(f"{number}. `{found['error']}`" for number, found in enumerate(result["diagnostics"], 1))
            lines.append("")
        else:
            lines.append("None.\n")
        if result["failed_commands"]:
            lines.append("### Failed `plenty` commands\n")
            for found in result["failed_commands"]:
                lines.append(fence("$ " + found["command"][:300] + "\n" + found["output"]))
        lines.append("### `plenty` commands\n")
        lines.extend(f"- `{command.splitlines()[0][:200]}`" for command in result["plenty_commands"])
        if result["leaks"]:
            lines.append("\n### Context leaks\n\nTool calls that reached outside the task directory and skill:\n")
            lines.extend(f"- `{leak}`" for leak in result["leaks"])
        lines.append("\n### Agent's summary\n\n" + (result["final_message"].strip() or "(none)") + "\n")
    (run_dir / "report.md").write_text("\n".join(lines))
    (run_dir / "report.json").write_text(json.dumps({"meta": meta, "results": results}, indent=2))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("tasks", nargs="*", help=f"tasks to run (default all: {', '.join(t.name for t in TASKS)})")
    parser.add_argument("--model", help="model passed to the agent (default: its own default)")
    parser.add_argument("--effort", help="reasoning effort passed to the agent")
    parser.add_argument("--max-budget-usd", type=float, default=3.0, help="per-task spending cap (default 3)")
    parser.add_argument("--timeout", type=int, default=1800, help="per-task agent timeout in seconds")
    parser.add_argument("--compiler", type=Path, default=REPO / "target/release/plenty")
    parser.add_argument("--agent", default="claude", help="Claude Code executable")
    parser.add_argument("--out", type=Path, help="report directory (default target/skill-trial/RUN)")
    parser.add_argument("--workdir-root", help="parent of task directories (default: system temp, outside any checkout)")
    parser.add_argument("--keep", action="store_true", help="keep each task directory")
    arguments = parser.parse_args(argv)
    arguments.workdir_root = arguments.workdir_root or default_workdir_root()

    by_name = {task.name: task for task in TASKS}
    unknown = [name for name in arguments.tasks if name not in by_name]
    if unknown:
        parser.error(f"unknown tasks {unknown}; choose from {sorted(by_name)}")
    selected = [by_name[name] for name in arguments.tasks] or TASKS
    compiler = arguments.compiler.resolve()
    if not compiler.is_file():
        raise SystemExit(f"no compiler at {compiler}; run `just compiler` first")
    if shutil.which(arguments.agent) is None:
        raise SystemExit(f"{arguments.agent} is not on PATH")

    run = datetime.datetime.now(datetime.UTC).strftime("%Y%m%dT%H%M%SZ")
    run_dir = (arguments.out or REPO / "target/skill-trial" / run).resolve()
    run_dir.mkdir(parents=True, exist_ok=False)
    version = subprocess.run([compiler, "--version"], capture_output=True, text=True).stdout.strip()
    # The published tree is what users install; a plugin directory loads it as
    # `/plugin install` would. Its path names "skill-trial-skill" so reads of
    # it are not reported as leaks.
    plugin_dir = Path(tempfile.mkdtemp(prefix="plenty-skill-trial-skill-", dir=arguments.workdir_root))
    skill = sync_agent_skill.build(plugin_dir / "plugin", sync_agent_skill.compiler_version())
    meta = {"run": run, "compiler_version": version, "model": arguments.model, "skill": str(skill)}
    results = []
    try:
        for task in selected:
            results.append(run_task(task, arguments, skill, run_dir, compiler))
            report(results, run_dir, meta)
    finally:
        shutil.copytree(skill, run_dir / "skill")
        shutil.rmtree(plugin_dir)
        meta["skill"] = "skill/"
        if results:
            report(results, run_dir, meta)
    print(run_dir / "report.md")
    return 0 if all(result["passed"] for result in results) else 1


if __name__ == "__main__":
    sys.exit(main())
