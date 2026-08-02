# lava-core

Typed primitive layer for the lava suite — a tatara-lisp + Rust DSL frontend
for [magma](https://github.com/pleme-io/magma).

*Lava* is the Brazilian-Portuguese name for the substance magma flows as: this
crate sits on magma the way `pangea-core` sits under the Pangea DSL, providing
the typed primitives every other `lava-*` crate composes.

## Install

```toml
[dependencies]
lava-core = "0.1"
```

## What's here

| Module | Purpose |
|---|---|
| `lib.rs` | The typed primitive surface — resources, references, identifiers |
| `synthesizer.rs` | Lowers the typed primitives toward magma's executor IR |

## The suite

```
lava-core ──┬──► lava-arch ──► lava-architectures
            ├──► lava-contracts
            └──► lava-eval ──► lava-runtime
lava-types ──► lava-schema ──┘
```

Every crate in the family is published to crates.io and depends on its
siblings by version, not by git revision.

## License

MIT
