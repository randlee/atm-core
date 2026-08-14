# AN.13 — released sc-compose 1.4.1 evidence

Recorded before the AN.13 durable catalog-format implementation.

| Evidence | Verified value |
| --- | --- |
| `sc-composer` registry package | `1.4.1`, checksum `4415ff74a7f91a7505a7c9fc464908ed5e0e684d2648b5d731e0533c371edb2c` |
| `sc-sha` registry package | `1.4.1`, checksum `01502b8bda56eef5c2f445a88396d75cc223c8ce91709ac007dbb81f40e577ba` |
| adapter manifest | exact `sc-composer =1.4.1`; no local path, git revision, or version range |
| released API used by ATM | `sc_composer::OutputFormat::from_template_path`, called only in `atm-template-sc-compose` |
| direct-library gate | [sc-compose #448](https://github.com/randlee/sc-compose/issues/448), closed 2026-08-13; its scope covers `compose` followed by `check_rendered_output` for malformed and valid JSON |

AN.13 consumes only the released path classification API to persist the
adapter-selected format. It does **not** invoke checked emission; AN.14 owns
the `check_rendered_output` runtime path.
