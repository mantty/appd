# appd Agent Instructions

appd is a cross-platform app framework designed to allow developers to create apps for mobile, desktop, and web from a single web-native codebase using vanilla fullstack web semantics and frameworks.

It aims to be broadly compatible with Cloudflare workers. Any app written for appd should work without changes on Cloudflare workers (though not all apps written for Cloudflare workers will necessarily run on appd). It supports frameworks such as Astro, NextJS, Hono, etc - all via Vite/Cloudflare.

## Simple is better than complex

Flat code structures are preferrable to nested ones.
Clear and descriptive names are better than clever ones, even if they are longer.
Never go beyond four levels of indentation.
If you have loops within loops, consider if a different data structure would avoid it.
After writing any code, ask yourself if it could be simpler. If it could be, it should be.

## Separation of concerns matters. A lot.

Above all else, consider what any area of code should be concerned with. What it should own. What it should know.
Preserving separation of concerns to appropriate, meaningful boundaries makes future maintenance simpler and avoids spaghetti code. Be strict in this regard. Any exceptions require user confirmation.

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

Both groups are 'developers', but with different needs.

Runtime compilation, rust tooling, infrastructure, SDK packaging, and
runtime-platform build support are contributor concerns.
Do not put those flows in the appd user CLI unless explicitly requested.
Do not conflate the needs of these two groups.
appd users should never have to consider the internals of appd and how it is made.
