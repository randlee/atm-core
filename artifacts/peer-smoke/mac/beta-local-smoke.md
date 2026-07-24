# Mac local smoke — 1.3.2-beta.1

Date: 2026-07-24

The managed singleton selected the matched `atm` and `atm-daemon`
`1.3.2-beta.1` release pair. `atm doctor --json` reported `healthy`, with
both client and daemon contexts at `1.3.2-beta.1`.

The capability-authenticated local HTTP request below returned the same healthy
doctor report:

```sh
curl -X GET "http://$(jq -r .ipv4_loopback ~/.atm/daemon/local-http.json)/v1/atm/doctor" \
  -H "X-ATM-Local-Capability: $(jq -r .capability_base64url ~/.atm/daemon/local-http.json)" \
  -H 'Content-Type: application/json' \
  --data "$(jq -nc --arg home \"$HOME\" --arg cwd \"$PWD\" \
    '{home_dir:$home,current_dir:$cwd,team_override:null,caller_team:\"atm-dev\",caller_identity:\"arch-ctm\"}')"
```

Canonical local write evidence:

- no-ack send: `01KYAJR2QEJFENTEQ6WMN4WP7R`
- requires-ack send: `01KYAJR34AYJ2VGKWS5AR5GF5A`
- reply ack: `01KYAJR3H8VF26JP2MVFKADJXH`

The test suite was run with the managed daemon stopped to preserve the
singleton rule, then the same managed service was restarted and verified with
`atm doctor --json`. `just lint` and `just test` passed.
