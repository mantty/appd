# appd Agent Instructions

## Contributor/User Boundary

appd has two distinct audiences:

- appd users: people using appd to build applications.
- appd contributors: people maintaining appd itself.

Workerd compilation, Bazel cache setup, infrastructure, SDK packaging, and
runtime-platform build support are contributor/build-infrastructure concerns.
Do not put those flows in the appd user CLI unless explicitly requested.
Do not conflate the needs of these two groups.
appd users should never have to consider the internals of appd and how it is made.
