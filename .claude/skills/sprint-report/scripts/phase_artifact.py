"""Publish the required phase diagram without touching another agent's checkout."""
from pathlib import Path
import subprocess
import tempfile

import repo_config


def git(repo, *args):
    result = subprocess.run(['git', *args], cwd=repo, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr.strip() or result.stdout.strip()}")
    return result.stdout.strip()


def publish_artifact(repo, branch, phase, html):
    """Commit only the rendered phase HTML on the integration branch.

    The canonical JSONL plan is a planner-owned source file.  Reporting must
    read it, never rewrite it from mutable Beads state.
    """
    if not branch:
        raise RuntimeError('Phase root has no integration_branch; use --output for a local export')
    git(repo, 'check-ref-format', '--branch', branch)
    # Use the fetched tracking ref, not whichever FETCH_HEAD another process may write.
    ref = f'refs/remotes/origin/{branch}'
    git(repo, 'fetch', '--no-tags', 'origin', f'refs/heads/{branch}:{ref}')
    head = git(repo, 'rev-parse', ref)
    relative = Path(repo_config.load(Path(repo))['plans_dir']) / f'phase-{phase}'
    html_path = relative / f'phase-{phase}-dag.html'
    with tempfile.TemporaryDirectory(prefix='sprint-report-artifact-') as directory:
        checkout = Path(directory) / 'checkout'
        git(repo, 'worktree', 'add', '--detach', str(checkout), head)
        try:
            (checkout / relative).mkdir(parents=True, exist_ok=True)
            (checkout / html_path).write_text(html)
            git(checkout, 'add', '--', str(html_path))
            if git(checkout, 'diff', '--cached', '--name-only'):
                git(checkout, 'commit', '-m', f'docs: refresh phase {phase} dependency diagram')
                # A normal push refuses a concurrent remote update; never force it.
                git(checkout, 'push', 'origin', f'HEAD:refs/heads/{branch}')
            commit = git(checkout, 'rev-parse', 'HEAD')
        finally:
            # This checkout was created solely for these generated artifacts.
            git(repo, 'worktree', 'remove', '--force', str(checkout))
    return {'branch': branch, 'commit': commit, 'html_path': str(html_path)}
