# appd API and conformance testing

Status: proposal

Second of three, after `appd-library.md`, so it locks a settled surface.

## Purpose

Make API changes easy to review and breaking changes automatic to detect, and
prove every platform shell behaves the same against the API.

## Surface artifacts

Generate an API listing for every public surface, commit it, and fail CI when
the code and the listing disagree.

| Surface | Tool |
|---|---|
| Rust crates | `cargo public-api` |
| C ABI | cbindgen |
| Kotlin shell library | binary-compatibility-validator |
| TypeScript packages | API Extractor |

Semver classification reads the same listings later. Plugin-facing packages
join as they are written.

## Conformance suite

One suite, run against every platform shell:

- lifecycle calls in each order the OS can produce
- event emission and ordering
- certificate challenge decisions
- startup failure reporting

A platform is supported when it passes.

## Done when

A public change without its regenerated listing fails CI, and every shell runs
the conformance suite.
