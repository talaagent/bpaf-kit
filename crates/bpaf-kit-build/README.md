# bpaf-kit-build

Dependency-free `build.rs` helper for
[bpaf-kit](https://github.com/talaagent/bpaf-kit): bakes the git rev,
dirty flag, build date and target into the binary for
`bpaf_kit::build_info!()`.

```rust
// build.rs
fn main() {
    bpaf_kit_build::emit();
}
```

Each value can be forced through the build environment
(`BPAF_KIT_GIT_REV`, `BPAF_KIT_GIT_DIRTY`, `BPAF_KIT_BUILD_DATE`,
`BPAF_KIT_TARGET`); `SOURCE_DATE_EPOCH` is honoured.

License: Apache-2.0.
