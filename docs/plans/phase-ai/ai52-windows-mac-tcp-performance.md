# AI.52 Windows and macOS TCP Performance Note

## Scope

This note compares the final AI.52 Windows TCP results with the latest tracked
macOS TCP results. It does not compare Windows TCP with the macOS UDS baseline;
those are different transports and remain separate evidence sets.

The Windows artifacts are the final error-free runs from source revision
`5d32095079821c7ecf53eb92e0cd9bf891edcaee`. The macOS TCP artifacts use source
revision `3ec7ce1ff7269d8f43a65658c712778abbf2de14`. All profiles use the same
public authenticated HTTP admission benchmark and 64 client workers.

## Results

| Frames per connection | macOS TCP median/s | Windows TCP median/s | Windows as % of macOS | Gap |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 11,847.59 | 4,077.04 | 34.4% | 65.6% |
| 2 | 19,674.03 | 5,888.18 | 29.9% | 70.1% |
| 8 | 25,729.48 | 4,281.17 | 16.6% | 83.4% |
| 16 | 25,841.74 | 4,068.24 | 15.7% | 84.3% |
| 64 | 24,958.69 | 1,002.83 | 4.0% | 96.0% |

Windows `f1`, `f2`, `f8`, and `f16` completed without request/response errors,
passed doctor/restart/durability checks, and met the local 1,000 admissions/s
floor. `f64` was also error-free and durable, but its median was only
`1,002.83/s` and the sustained profile contained under-floor intervals, so it
remains failed evidence rather than a correctness failure.

## Root Cause

`frames_per_connection` is keep-alive depth, not SIMD width or parallelism.
The benchmark client sends bounded batches of up to eight requests, but the
Windows TCP handler processes each request synchronously: read, route, write,
then read the next request. The Unix/macOS local TCP path uses the shared
dispatch worker pool and can enqueue up to eight keep-alive requests before
waiting for responses. Thus macOS benefits from per-connection pipelining at
`f8`/`f16`, while Windows does not.

The curve is therefore a Windows TCP transport-path limitation, amplified by
different host hardware. It is not evidence of SIMD execution, and it should
not be described as a pure Windows-versus-macOS operating-system comparison.
Making Windows use equivalent bounded in-flight dispatch would be a deliberate
transport redesign and is outside AI.52's benchmark-confirmation scope.

## Source Artifacts

- Windows: `site/reports/send-message-benchmark/20260801-202541.596229-windows-x64-01-tcp-f1.json`
- Windows: `site/reports/send-message-benchmark/20260801-202607.096533-windows-x64-01-tcp-f2.json`
- Windows: `site/reports/send-message-benchmark/20260801-202633.208353-windows-x64-01-tcp-f8.json`
- Windows: `site/reports/send-message-benchmark/20260801-202659.253143-windows-x64-01-tcp-f16.json`
- Windows: `site/reports/send-message-benchmark/20260801-202725.739664-windows-x64-01-tcp-f64.json`
- macOS TCP artifacts: `site/reports/send-message-benchmark/20260801-072723.571920` through `20260801-072846.375494`
