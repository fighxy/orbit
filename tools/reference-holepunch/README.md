# Holepunch reference inventory

`package.json` lists the Holepunch packages the project originally planned to
use. Nothing in Orbit imports them: the client runs on the Rust core
(`crates/`) and Kotlin Multiplatform (`shared/`, `client/`, `apps/`). Node and
Bare are not runtime dependencies.

The list is kept only as a reference for studying upstream behaviour, alongside
[docs/holepunch-map.md](../../docs/holepunch-map.md). Install it here if an
experiment needs the packages; never from the repository root.
