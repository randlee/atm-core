#!/usr/bin/env python3
"""Install an opt-in immutable sc-lint checkout, binaries and Python wheel."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

BINARIES = ('sc-lint', 'sc-lint-boundary', 'sc-lint-portability', 'sc-lint-runtime')
MATURIN_VERSION = '1.9.4'
SETUP_TIMEOUT_SECONDS = 300
BUILD_TIMEOUT_SECONDS = 1800
PROBE_TIMEOUT_SECONDS = 30


def revision(value: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{40}', value):
        raise ValueError('source revision must be a full lowercase 40-character commit SHA')
    return value


def selected_revision(explicit: str, manifest: Path) -> str:
    configured = ''
    if manifest.is_file() and 'sc_lint_source_revision' in manifest.read_text(encoding='utf-8'):
        import tomllib  # Only opt-in source manifests require Python 3.11+.
        project = tomllib.loads(manifest.read_text(encoding='utf-8')).get('project', {})
        if 'sc_lint_source_revision' in project:
            configured = revision(project['sc_lint_source_revision'])
    if explicit:
        revision(explicit)
    if explicit and configured and explicit != configured:
        raise ValueError('action source revision differs from rendered manifest')
    return explicit or configured


def python_path(root: Path, windows: bool = os.name == 'nt') -> Path:
    return root / ('Scripts/python.exe' if windows else 'bin/python3')


def run(command, cwd: Path, env=None, timeout: float = SETUP_TIMEOUT_SECONDS) -> str:
    arguments = [str(x) for x in command]
    try:
        result = subprocess.run(arguments, cwd=cwd, env=env, check=True,
                                stdout=subprocess.PIPE, stderr=sys.stderr, text=True,
                                encoding='utf-8', timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f'sc-lint source command timed out after {timeout}s: {arguments!r}') from error
    return result.stdout.strip()


def checkout(url: str, sha: str, destination: Path) -> None:
    revision(sha)
    destination.mkdir()
    run(['git', 'init', '-q'], destination)
    run(['git', 'remote', 'add', 'origin', url], destination)
    run(['git', 'fetch', '--depth', '1', 'origin', sha], destination)
    run(['git', '-c', 'core.autocrlf=false', 'checkout', '--detach', 'FETCH_HEAD'], destination)
    verify_checkout(destination, sha)


def verify_checkout(source: Path, sha: str) -> None:
    if run(['git', 'rev-parse', 'HEAD'], source) != revision(sha):
        raise ValueError('checked-out source does not match requested commit')
    run(['git', 'diff', '--exit-code', 'HEAD', '--'], source)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def install(repository: str, sha: str, workspace: Path, temporary: Path) -> dict:
    revision(sha)
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('repository must be a GitHub owner/repository slug')
    if (workspace / '.sc-lint/venv').exists():
        raise ValueError('source installation requires a fresh consumer .sc-lint/venv')
    staging = Path(tempfile.mkdtemp(prefix='sc-lint-source-', dir=temporary))
    source = staging / 'source'
    checkout(f'https://github.com/{repository}.git', sha, source)
    # A private target prevents a prior release/source build from supplying binaries.
    environment = dict(os.environ, CARGO_TARGET_DIR=str(staging / 'target'))
    metadata = json.loads(run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], source, environment))
    versions = {p['name']: p['version'] for p in metadata['packages']}
    expected = versions['sc-lint']
    if any(versions.get(name) != expected for name in (*BINARIES, 'sc-lint-py')):
        raise ValueError('CLI, sibling backends and Python crate versions differ in source')
    run(['cargo', 'build', '--locked', '--release', *[arg for name in BINARIES for arg in ('--bin', name)]], source, environment, timeout=BUILD_TIMEOUT_SECONDS)
    build_venv = staging / 'build-venv'
    run([sys.executable, '-m', 'venv', build_venv], staging)
    builder = python_path(build_venv)
    run([builder, '-m', 'pip', 'install', '--disable-pip-version-check', f'maturin=={MATURIN_VERSION}'], staging)
    wheels = staging / 'wheels'
    run([builder, '-m', 'maturin', 'build', '--locked', '--release', '--manifest-path',
         source / 'bindings/sc-lint-py/Cargo.toml', '--out', wheels, '--interpreter', builder], source, environment, timeout=BUILD_TIMEOUT_SECONDS)
    archives = list(wheels.glob('*.whl'))
    if len(archives) != 1:
        raise ValueError('source build must produce exactly one sc-lint wheel')
    verify_checkout(source, sha)
    install_dir = staging / 'bin'
    install_dir.mkdir()
    for name in BINARIES:
        filename = name + ('.exe' if os.name == 'nt' else '')
        shutil.copy2(staging / 'target/release' / filename, install_dir / filename)
    cli = install_dir / ('sc-lint.exe' if os.name == 'nt' else 'sc-lint')
    value = json.loads(run([cli, 'version', '--json'], workspace, timeout=PROBE_TIMEOUT_SECONDS))
    if value.get('ok') is not True or value.get('data', {}).get('version') != expected or value.get('data', {}).get('status') != 'pass':
        raise ValueError('built CLI version does not match checked source metadata')
    # A pre-existing environment could retain helpers from another revision.
    # Refuse it rather than mixing or silently falling back to a published wheel.
    venv = workspace / '.sc-lint/venv'
    if venv.exists():
        raise ValueError('source installation requires a fresh consumer .sc-lint/venv')
    run([sys.executable, '-m', 'venv', venv], workspace)
    consumer_python = python_path(venv)
    # sc-lint itself is the exact local wheel. Its declared third-party deps may
    # come from the configured package index; no sc-lint version lookup occurs.
    run([consumer_python, '-m', 'pip', 'install', '--disable-pip-version-check', archives[0]], workspace)
    code = 'import json,sc_lint; print(json.dumps({"version":sc_lint.__version__,"path":sc_lint.__file__}))'
    installed = json.loads(run([consumer_python, '-I', '-c', code], workspace, timeout=PROBE_TIMEOUT_SECONDS))
    if installed['version'] != expected or not Path(installed['path']).resolve().is_relative_to(venv.resolve()):
        raise ValueError('installed Python package version/path differs from checked source')
    record = {'source_revision': sha, 'repository': repository, 'version': expected,
              'wheel_sha256': digest(archives[0]), 'wheel': str(archives[0]),
              'binaries': {p.name: digest(p) for p in install_dir.iterdir()},
              'binary_directory': str(install_dir), 'python': str(consumer_python)}
    (workspace / '.sc-lint/source-install.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    return record


def report_lint_smoke(value: dict) -> None:
    print(json.dumps(value, indent=2, sort_keys=True))
    if value.get('ok') is not True or value.get('error', {}).get('code') == 'CLI.CONFIG_ERROR':
        raise ValueError('source sc-lint root discovery failed')
    print('Root discovery/backend execution succeeded; reported lint status=' + str(value.get('data', {}).get('status')))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('resolve', 'install', 'smoke'))
    args = parser.parse_args()
    workspace = Path(os.environ['GITHUB_WORKSPACE']).resolve()
    sha = selected_revision(os.environ.get('SC_LINT_SOURCE_REVISION_INPUT', ''), workspace / 'release/publish-artifacts.toml')
    if not sha and (workspace / '.sc-lint/source-install.json').exists():
        raise ValueError('release installation cannot reuse a source-installed consumer')
    if args.operation == 'smoke':
        value = json.loads(run([os.environ['SC_LINT_BIN'], '--json', '--root', workspace, 'lint', 'sc-boundary'], workspace))
        report_lint_smoke(value)
        return
    if args.operation == 'resolve':
        with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as stream:
            stream.write(f'SC_LINT_SOURCE_REVISION={sha}\n')
        return
    if not sha:
        raise ValueError('source install requires an explicit immutable revision')
    record = install(os.environ['SC_LINT_REPOSITORY'], sha, workspace, Path(os.environ['RUNNER_TEMP']))
    with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as stream:
        stream.write(record['binary_directory'] + '\n')
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as stream:
        stream.write('SC_LINT_BIN=' + str(Path(record['binary_directory']) / ('sc-lint.exe' if os.name == 'nt' else 'sc-lint')) + '\n')
    print(json.dumps(record, sort_keys=True))


if __name__ == '__main__':
    main()
