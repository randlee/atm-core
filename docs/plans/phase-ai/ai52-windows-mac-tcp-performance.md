# AI.52 Windows and macOS TCP Performance Note

## Scope

This note compares the final AI.52 Windows TCP results with the latest tracked
macOS TCP results. It does not compare Windows TCP with the macOS UDS baseline;
those are different transports and remain separate evidence sets.

The final Windows artifacts are the error-free runs from source revision
`fd8dd58e04a8844148e0e4faa3a7df7ece9956c9`. The macOS TCP artifacts use source
revision `3ec7ce1ff7269d8f43a65658c712778abbf2de14`. All profiles use the same
public authenticated HTTP admission benchmark and 64 client workers.

## Results

| Frames per connection | macOS TCP median/s | Windows TCP median/s | Windows as % of macOS | Gap |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 11,847.59 | 4,053.94 | 34.2% | 65.8% |
| 2 | 19,674.03 | 6,463.69 | 32.9% | 67.1% |
| 8 | 25,729.48 | 8,269.01 | 32.1% | 67.9% |
| 16 | 25,841.74 | 8,636.26 | 33.4% | 66.6% |
| 64 | 24,958.69 | 8,218.29 | 32.9% | 67.1% |

All five profiles completed without request/response errors, passed
doctor/restart/durability checks, and met the local 1,000 admissions/s floor.
The earlier `f64` artifact from
`5d32095079821c7ecf53eb92e0cd9bf891edcaee` (median `1,002.83/s`,
`passed: false`) is superseded by the two later passing `f64` runs, including
the final `20260801-224829.996132-windows-x64-01-tcp-f64.json` artifact
(`8,218.29/s`, `passed: true`). It is retained as historical failed evidence,
not a final-result claim.

## Root Cause

`frames_per_connection` is keep-alive depth, not SIMD width or parallelism.
The final Windows set remains below the macOS host's TCP medians, but it is
error-free and above the sprint floor for every profile. The superseded low
`f64` result does not establish a Windows transport-path limitation. A causal
comparison requires a dedicated profiling/reproduction sprint; AI.52 records
confirmation evidence only.

## Source Artifacts

- Windows: `site/reports/send-message-benchmark/20260801-224646.543953-windows-x64-01-tcp-f1.json`
- Windows: `site/reports/send-message-benchmark/20260801-224713.639733-windows-x64-01-tcp-f2.json`
- Windows: `site/reports/send-message-benchmark/20260801-224738.771641-windows-x64-01-tcp-f8.json`
- Windows: `site/reports/send-message-benchmark/20260801-224804.795757-windows-x64-01-tcp-f16.json`
- Windows: `site/reports/send-message-benchmark/20260801-224829.996132-windows-x64-01-tcp-f64.json`
- macOS TCP artifacts: `site/reports/send-message-benchmark/20260801-072723.571920` through `20260801-072846.375494`
