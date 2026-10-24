---
title: Tool Sender Trust Root
audience: end-user
reviewed_for_release: 1.6.2
---

# Tool Sender Trust Root

Virtual `@tool` senders are enabled only by the daemon's administrator-owned
trust root. The daemon reads exactly `/etc/atm/tool-sender-trust-root.json` on
Unix. It does not read this policy from an ATM repository, a user home
directory, a command-line option, a hook, or an environment variable.

On Unix, the `/etc/atm` directory must be owned by `root` and not writable by
group or other users. The document must be a root-owned regular file with no
group or world write permission. The policy contains public keys, not private
key material, so it may be readable by the unprivileged daemon account (for
example, mode `0644`). If these integrity checks, the JSON schema, or any key
validation fails, signed installer and peer tool-sender admission fail closed.

On Windows, the daemon reads the fixed system location
`%ProgramData%\ATM\tool-sender-trust-root.json`. The `ATM` directory and the
file must reject reparse points and have explicit protected ACLs: only `SYSTEM`
and `BUILTIN\Administrators` may modify them, while the daemon identity has
read/traverse access only. Neither location is configurable by a request,
environment variable, repository setting, or hook payload.

## Provisioning

The trust document names the daemon's durable local host identity, approved
installer keys and their sender allowlists, plus peer-origin keys and their
sender allowlists. All fields are required; an empty installer or peer-origin
policy is rejected.

```json
{
  "version": 1,
  "host_id": "daemon.example",
  "installers": [
    {
      "installer_id": "release-installer",
      "key_id": "2026-10",
      "public_key": "BASE64_ED25519_PUBLIC_KEY",
      "allowed_senders": ["release-bot"]
    }
  ],
  "trusted_origins": [
    {
      "host_id": "publisher.example",
      "public_key": "BASE64_ED25519_PUBLIC_KEY",
      "allowed_senders": ["release-bot"]
    }
  ]
}
```

The public keys use padded standard base64 and encode Ed25519 public keys.
`host_id` and every sender identifier are validated by ATM before the policy
is accepted. The local `host_id` is part of signed registration validation;
set it to the daemon's stable host name, not a request-provided peer name.

## Rotation

Install a complete replacement document with the intended old and new keys
and allowlists, set its owner and mode before activation, then atomically
rename it into `/etc/atm/tool-sender-trust-root.json`. Never edit the live
file in place. A daemon only accepts complete validated snapshots; a malformed
or unsafe replacement does not weaken the current admission policy.

Keep an old installer or origin key only for the period in which it must
validate already-authorized traffic. Remove it only after dependent senders
and peer hosts have moved to the replacement key. A policy change does not
turn an arbitrary CLI `--from`, environment variable, or mTLS peer identity
into authority for a virtual sender.
