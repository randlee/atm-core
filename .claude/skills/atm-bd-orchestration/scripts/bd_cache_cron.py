"""ATM cron module for bd-cache: drain journaled writes, sync active phases, escalate (#557).

Register once per repository (ADR-069), from the beads repository root:

    atm cron start bd-cache --phase <current-phase> --cron "*/30 * * * * *" --until-hook \
      --module <skill>/scripts/bd_cache_cron.py --cwd <repo-root> --custom '{"team": "<team>"}'

Each tick drains pending writes, then syncs every phase whose `.atm-bd/phase-<x>.toml`
root bead is open (`custom.phases`, a list, limits this to the named phases). Every escalated write, and each new sync failure cause, is sent
once to that phase's `lead` through `atm send --team tool` as `atm-bd@tool`; a backend outage is sent once to every active
phase's lead, not as per-phase sync failures (bd_cache.UNREACHABLE); an undeliverable
escalation stays unannounced and is retried next tick. The schedule stops when no
phase is active and the journal holds no pending or unannounced entries.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bd_cache  # noqa: E402

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    tomllib = None

TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 30000
TICK_BUDGET_S = 25.0
ATM = os.environ.get("ATM_BIN", "atm")


def phases(root, only=None):
    """{phase: {"root": id, "lead": name}} from `.atm-bd/phase-*.toml`; `only` limits it to the named phases."""
    found = {}
    for path in sorted((root / ".atm-bd").glob("phase-*.toml")):
        if tomllib is None:
            raise bd_cache.CacheError("BD_CACHE.NO_TOML", "Python 3.11+ (tomllib) is required to read phase files",
                                      "run the cron module with Python 3.11 or newer")
        data = tomllib.loads(path.read_text())
        name = path.stem[len("phase-"):]
        if data.get("root") and data.get("lead") and (only is None or name in only):
            found[name] = {"root": data["root"], "lead": data["lead"]}
    return found


def active_phases(cache, configured):
    """Phases whose root bead is open; roots are read in one batched `bd list --id` when not cached."""
    missing = [p["root"] for p in configured.values() if cache.cached(p["root"]) is None]
    if missing:
        try:
            rows = cache.bd_rows(missing)
        except bd_cache.CacheError as error:
            if error.code not in bd_cache.UNREACHABLE:
                raise
            cache.record_outage(error)  # an uncached root is treated as inactive until bd answers
            rows = {}
        for row in rows.values():
            cache.store(row)
    return {name: info for name, info in configured.items()
            if (cache.cached(info["root"]) or {}).get("status") not in (None, "closed")}


def send(team, lead, body):
    # atm-bd@tool is the installer's notification member; cron@tool is atm's daemon-owned scheduler principal and
    # refuses ordinary sends.
    env = {**os.environ, "ATM_IDENTITY": "atm-bd", "ATM_TEAM": "tool"}
    result = subprocess.run([ATM, "send", f"{lead}@{team}", "--team", "tool", "--stdin", "--json"], input=body,
                            capture_output=True, text=True, check=False, timeout=10, env=env)
    return result.returncode == 0, (result.stderr or result.stdout).strip()[-300:]


def escalation_text(entry):
    return (f"bd-cache escalation: journaled write #{entry['seq']} ({entry['op']} on {entry['bead']}, by "
            f"{entry['actor']}) was not applied: {entry['last_error']}. Check the bead in bd (a timed-out write may "
            f"have landed), set the intended state with bd, then clear the entry: bd_cache.py resolve {entry['seq']} --note <what you did> --actor <you>.")


def outage_text(outage):
    return (f"bd-cache: the beads backend has been unreachable since {outage['since']}: {outage['error']}. "
            "Journaled writes stay pending and count no retry attempts; they apply when bd answers again, and hooks "
            "read the last synced rows meanwhile. Fix: restore bd and its dolt server on this host (`bd version`, "
            "`bd list -n 1 --json`). This is sent once per outage.")


def tick(root, custom, *, cache=None, send_fn=send, clock=time.monotonic):
    deadline = clock() + TICK_BUDGET_S
    team = custom.get("team")
    if not team:
        return {"result": "refused", "message": "bd-cache cron needs custom.team (the ATM team of the phase leads)"}
    only = custom.get("phases")
    if only is not None and not (isinstance(only, list) and all(isinstance(p, str) for p in only)):
        return {"result": "refused", "message": "custom.phases must be a list of phase names"}
    cache = cache or bd_cache.BdCache(root)
    configured = phases(root, only)
    report = {"drain": cache.drain(deadline=deadline - 5.0, monotonic=clock), "sync": {}, "undelivered": []}
    active = active_phases(cache, configured)
    for name in active:
        if clock() >= deadline - 2.0:
            report["sync"][name] = "deferred: tick budget"
            continue
        try:
            report["sync"][name] = cache.sync(name)
            cache.clear_sync_alert(name)
        except bd_cache.CacheError as error:
            report["sync"][name] = error.as_json()
            if bd_cache.UNREACHABLE & {error.code, error.extra.get("cause")}:
                continue  # the outage is announced once below, not once per phase
            cause = f"{error.code}: {error.detail}"
            if cache.sync_alert(name) != cause:
                ok, detail = send_fn(team, active[name]["lead"],
                                  f"bd-cache sync of phase {name} is failing: {cause}. Fix: {error.fix}. Hooks keep "
                                  "reading the last synced rows until it recovers.")
                if ok:
                    cache.set_sync_alert(name, cause)
                else:
                    report["undelivered"].append({"phase": name, "error": detail})
    outage = cache.outage()
    if outage and not outage["announced_at"]:
        leads = sorted({info["lead"] for info in (active or configured).values()})
        failed = [(lead, detail) for lead in leads
                  for ok, detail in [send_fn(team, lead, outage_text(outage))] if not ok]
        if leads and not failed:
            cache.mark_outage_announced()
        report["undelivered"] += [{"outage": outage["since"], "lead": lead, "error": detail} for lead, detail in failed]
        if not leads:
            report["undelivered"].append({"outage": outage["since"], "error": "no phase lead to tell"})
    for entry in cache.unannounced_escalations():
        lead = (configured.get(entry["phase"]) or {}).get("lead")
        if not lead:
            report["undelivered"].append({"seq": entry["seq"], "error": f"no lead for phase {entry['phase']!r}"})
            continue
        ok, detail = send_fn(team, lead, escalation_text(entry))
        if ok:
            cache.mark_announced(entry["seq"])
        else:
            report["undelivered"].append({"seq": entry["seq"], "error": detail})
    if report["undelivered"]:
        return {"result": "refused", "message": json.dumps(report["undelivered"])[:1000]}
    return {"result": "pass-through"}


def should_stop(root, cache=None, only=None):
    cache = cache or bd_cache.BdCache(root)
    if active_phases(cache, phases(root, only)):
        return False
    return not cache.has_open_work()


def main():
    request = json.load(sys.stdin)
    hook = sys.argv[1] if len(sys.argv) > 1 else request.get("hook", "")
    root = Path.cwd()
    try:
        if hook == "cron.tick":
            reply = tick(root, request.get("custom") or {})
        elif hook == "cron.stop":
            stop = should_stop(root, only=(request.get("custom") or {}).get("phases"))
            reply = {"stop": stop, "reason": "no active phase and no open journal work"} if stop else {"stop": False}
        elif hook == "doctor":
            status = bd_cache.BdCache(root).status()
            reply = {"success": True, "message": f"bd-cache ready at {status['db']}", "findings": status["escalated"]}
        else:
            reply = {"result": "not-applicable"}
    except bd_cache.CacheError as error:
        reply = {"stop": False} if hook == "cron.stop" else {"result": "refused", "message": json.dumps(error.as_json())}
    print(json.dumps(reply))


if __name__ == "__main__":
    main()
