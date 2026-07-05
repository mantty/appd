# appd Agent Instructions

## Comments and Documentation

Comments and docs state current facts, tersely. One clause, present tense, only where the code cannot say it itself.

Never write:

- Narration or persuasion ("built for exactly this scenario", "matching what production does")
- Evidence trails ("confirmed via...", "its source comment cites...")
- History ("previously...", "this replaces...")
- Alternatives not taken, or anything else the code doesn't do

Bad: "WebAssembly still works there through DrumBrake, V8's own wasm interpreter, built for exactly this scenario -- its source comment cites tvOS, where JIT is unavailable."
Good: "jitless WebAssembly is supported via V8's own wasm interpreter, DrumBrake."

## Contributor/User Boundary

appd has two distinct audiences:

- appd users: people using appd to build applications.
- appd contributors: people maintaining appd itself.

Workerd compilation, Bazel cache setup, infrastructure, SDK packaging, and
runtime-platform build support are contributor/build-infrastructure concerns.
Do not put those flows in the appd user CLI unless explicitly requested.
Do not conflate the needs of these two groups.
appd users should never have to consider the internals of appd and how it is made.
