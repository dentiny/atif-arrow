# Harbor Hub → ATIF → Arrow

Run from the workspace root:

```sh
cargo run -p opendal-service-harborhub --example harborhub
```

The example reads a public Terminal-Bench trial from the job featured by
[Harbor Hub](https://hub.harborframework.com/):

- [Job](https://hub.harborframework.com/jobs/0f01715e-2f98-40e3-836f-1ac4fdce39a4?tab=results): Terminal-Bench 4.0.0, GPT-6 Astra / Codex.
- Trial: `foodstuff-beta-activity__53bc4621` (`d4f5439d-8367-467f-938a-005585690681`).
- Object: `results/trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json`.
- Agent: `codex` 0.151.0; model: `gpt-6-astra`.

Verified on 2026-10-06: **ATIF-v1.7 → 1 Arrow row, 13 columns, 17 steps,
15 tool calls**. The original JSON is 65,886 bytes; its SHA-256 is
`208c8399d78b49d9b210fde24814d72f9f85dec7ceb27aa89b204d10dba0f545`.
The object is served as `application/octet-stream`; its payload is ATIF JSON.

The example uses the production Harbor backend and `TrajectoryReader`, asserts the
fixed schema, provenance, agent identity, step/tool-call counts, prompt-token total,
and raw JSON byte length, then prints the schema. Arrow's JSON writer is used only
to inspect the converted values. See [the schema contract](../docs/schema.md).

It requires network access and uses Harbor's default public gateway key, with no
personal credentials. The live source can change or disappear, so this is an
explicit example run; regular workspace tests continue to use mocked HTTP responses.
This sample validates a direct ATIF trajectory; other trial artifacts and trajectories
stored only inside archives require separate handling.
