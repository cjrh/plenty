"""Build and link (never launch) the book's `plenty build` fences."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


def listings(book):
    for page in sorted(book.rglob('*.md')):
        language = None
        for number, line in enumerate(page.read_text().splitlines(), 1):
            if language is None and line.startswith('```'):
                language, start, lines = line[3:], number, []
            elif language is not None and line == '```':
                if language == 'plenty build':
                    yield page, start, '\n'.join(lines) + '\n'
                language = None
            elif language == 'plenty build':
                lines.append(line)
        if language == 'plenty build':
            raise ValueError(f'{page}:{start}: unterminated build fence')


def package_target(repo, page, include):
    source = (page.parent / include).resolve(strict=True)
    source.relative_to(repo)
    for package in source.parents:
        if not package.is_relative_to(repo):
            break
        manifest = package / 'plenty.toml'
        if not manifest.is_file():
            continue
        data = tomllib.loads(manifest.read_text())
        targets = data.get('bin', [{'name': data['package']['name'], 'path': 'src/main' + source.suffix}])
        for target in targets:
            if (package / target['path']).resolve() == source:
                return package, target['name']
        break
    raise ValueError(f'{source} is not a package executable entry point')


def build(repo, compiler):
    built = set()
    for page, line, body in listings(repo / 'book/src'):
        location = f'{page.relative_to(repo)}:{line}'
        with tempfile.TemporaryDirectory() as scratch:
            try:
                include = re.fullmatch(r'\{\{#include ([^\n{}]+)\}\}\n', body)
                if include:
                    package, target = package_target(repo, page, include[1])
                else:
                    package, target = Path(scratch), 'listing'
                    (package / 'src').mkdir()
                    (package / 'src/main.plenty').write_text(body)
                    (package / 'plenty.toml').write_text('manifest-version = 1\n[package]\nname = "listing"\nversion = "0.1.0"\n')
                print(f'{location}: building {target}', flush=True)
                if package not in built:
                    args = [compiler, 'build', '--offline'] + (['--locked'] if include else [])
                    subprocess.run(args, cwd=package, check=True)
                    if include:
                        built.add(package)
                if not (package / 'target/optimized' / target).is_file():
                    raise ValueError(f'missing linked executable: {target}')
            except (OSError, ValueError, subprocess.CalledProcessError) as e:
                raise SystemExit(f'{location}: {e}') from e


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plenty', type=Path, required=True)
    args = parser.parse_args()
    build(Path(__file__).resolve().parents[1], args.plenty.resolve(strict=True))
