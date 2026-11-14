#!/usr/bin/env python3
"""Add three zero-step instruction controls to an existing native result run."""
import importlib.util
import json
from pathlib import Path
import statistics

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('native', HERE / 'native.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
work = native.ROOT / 'target/declared-vectors'
path = work / 'native-results.json'
data = json.loads(path.read_text())
controls = {}
for label in data['instructions']:
    group, name = label.split('-', 1)
    binary = work / 'native' / label if name.startswith('native-') else work / 'nbody-source' / name
    controls[label] = [native.count(binary, 0, data['core'], 'instructions:u')[0] for _ in range(3)]
data['zero_instruction_counts'] = controls
data['incremental_instructions_per_step'] = {
    label: (statistics.median(data['instructions'][label]) - statistics.median(values)) / data['steps']
    for label, values in controls.items()}
path.write_text(json.dumps(data, indent=2) + '\n')
print(json.dumps(data['incremental_instructions_per_step'], indent=2))
