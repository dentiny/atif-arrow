# Tool-result reference benchmark

`cargo bench -p atif-arrow --bench tool_references -- 1024 100` validates and
converts 100 trajectories, each containing 1024 calls and 1024 results in reverse
order. Only `trajectory_id` is projected, but all fields and references are still
validated. The generated input and warm-up are outside the measured interval.

Run with 8, 32, 256 and 1024 calls to inspect scaling. Compare identical release
builds on the same machine, and repeat each process at least three times. This
benchmark includes JSON decoding and schema validation, so the total-time
improvement is smaller than the improvement in reference lookup alone.

End-to-end measurements in open-arrow-access (macOS arm64, Rust 1.97.1,
64 trajectories per scan, seven scans per process, five alternating A/B
processes, projected ID while validating all fields):

| Calls/results per trajectory | Before ms | After ms |
| --- | ---: | ---: |
| 8 | 1.210 | 1.116 |
| 32 | 4.326 | 4.309 |
| 256 | 37.014 | 24.134 |
| 1024 | 314.939 | 94.266 |

The 1024-call workload improves by 3.34×. Smaller rows are controls; no general
small-input improvement is claimed. CPU sampling finds `validate_details` in
1688/2477 main-thread samples before and 159/2463 after. Raw process measurements
and the complete setup are recorded in the open-arrow-access benchmark PR.
