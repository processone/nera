#!/usr/bin/env python3
"""Explicit BEAM parse-transform preparation for native Erlang dependencies.

Dependencies and their transform modules must already be built on BEAM. Generated
Erlang sources stay in a separate directory and can be consumed as a Nera path
dependency. BEAM is used at build time only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def prepare_dependency(source, beam_paths, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output or source in output.parents:
        raise ValueError('prepared output must be outside the dependency checkout')
    if output.exists() and any(output.iterdir()):
        raise ValueError('prepared output must be empty')
    files = sorted((source/'src').rglob('*.erl'))
    if not files:
        raise ValueError('dependency has no src/*.erl sources')
    if len({path.stem for path in files}) != len(files):
        raise ValueError('duplicate Erlang source module filenames')
    generated = output/'src'
    generated.mkdir(parents=True)
    env = os.environ.copy()
    for name in ('ERL_FLAGS', 'ERL_AFLAGS', 'ERL_ZFLAGS', 'ERL_LIBS'):
        env.pop(name, None)
    command = ['erlc', '-W0', '-P', '-o', str(generated)]
    for path in beam_paths:
        command.extend(['-pa', str(Path(path).resolve())])
    command.extend(['-I', str(source/'include'), *map(str, files)])
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise RuntimeError('BEAM parse-transform preparation failed:\n'+result.stdout+result.stderr)
    records = []
    for original in files:
        listing = generated/(original.stem+'.P')
        target = generated/original.name
        if not listing.is_file():
            raise RuntimeError('missing transformed source: '+str(listing))
        listing.rename(target)
        records.append({'original': str(original),
                        'original_sha256': hashlib.sha256(original.read_bytes()).hexdigest(),
                        'prepared': str(target.relative_to(output)),
                        'prepared_sha256': hashlib.sha256(target.read_bytes()).hexdigest()})
    manifest = {'method': 'erlc -P (BEAM parse transforms and preprocessing)',
                'source': str(source), 'command': command, 'sources': records,
                'stdout': result.stdout, 'stderr': result.stderr}
    (output/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--beam-path', type=Path, action='append', default=[])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    manifest = prepare_dependency(args.source, args.beam_path, args.output)
    print(f"Prepared {len(manifest['sources'])} modules in {args.output}")


if __name__ == '__main__':
    main()
