# Compile speed

`dbota_particles` compiles particle simulation, rendering, and model and light pools separately
from the client. The adapter in `src/particles/mod.rs` supplies asset handles, ground queries,
camera state, replay time and loading progress. Private implementation edits can reuse compiled
consumer code; public API or type changes may rebuild consumers.

Plain `release` keeps Cargo's original product settings. Agents can explicitly select
`PROFILE=dev_release tools/build.sh` for app-only incremental rebuilding; dependencies compile
whole. That tool builds the matching server beside the client with its release settings.
`PROFILE=dist tools/build.sh` builds the distribution with incremental compilation disabled,
thin LTO and one codegen unit; shipping browser builds inherit those settings. Test profiles
retain their own checks.

To measure a rebuild, keep the toolchain, dependency graph, target directory, jobs and available
resources fixed. Disable compiler caching and warm the selected graph once before timing:

```sh
RUSTC_WRAPPER= CARGO_BUILD_JOBS=8 cargo build --profile dev_release -p dbota --locked --offline --timings
```

Make a real private implementation edit in `src/runes.rs` or
`crates/dbota_particles/src/sim.rs`, repeat that command, then restore the exact original content
and rebuild again. Record command wall time and Cargo's report under `target/cargo-timings`.
Exclude the initial warm build. `--offline` requires dependencies to be downloaded already.

Measure an unchanged-content `touch` separately: incremental compilation may reuse code after
that touch, so its result does not establish the cost of a real system edit. To attribute a gain
to the crate boundary, compare the split with a control containing the same particle source
bodies in the app crate, using the same engine, dependencies and build settings. Unit durations
can overlap and must not be summed as whole-build wall time.
