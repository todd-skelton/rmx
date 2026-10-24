# cli

Command-line interface for creating and managing rsmix projects. It is Remix's `@remix-run/cli`, rewritten in Rust, which nobody asked for.

## Features

- Create new rsmix projects with `rsmix new`
- Build and run the app, rebuilding and reloading on change, with `rsmix dev`
- Build the optimized client and the release server with `rsmix build`
- List browser-reachable files and inspect URL mappings with `rsmix assets`
- Print shell completion scripts with `rsmix completion`
- Check the toolchain and rsmix app conventions with `rsmix doctor`
- Apply available low-risk project fixes with `rsmix doctor --fix`
- Inspect the current app route tree with `rsmix routes`
- Run project tests with `rsmix test`
- Print the current rsmix version with `rsmix version`
- Scaffold a starter app that matches the Remix project layout conventions

There is no `rsmix db`: databases are well served by existing crates such as `sqlx` and `diesel`, and there is no `rsmix.json` yet.

## Installation

Install the `rsmix` command with cargo. Only [rustup](https://rustup.rs) is needed beforehand:

```sh
cargo install rsmix-cli
```

The first `rsmix dev` or `rsmix build` installs what the browser build needs: the `wasm32-unknown-unknown` target through rustup, plus pinned releases of `wasm-bindgen` and binaryen's `wasm-opt`, checked against their published checksums and cached under `RSMIX_CACHE_DIR` (default: the platform cache directory). Set `RSMIX_WASM_BINDGEN` or `RSMIX_WASM_OPT` to use your own.

## Shell completion

Install bash completion:

```sh
rsmix completion bash >> ~/.bashrc
```

Install zsh completion:

```sh
rsmix completion zsh >> ~/.zshrc
```

## Usage

```sh
rsmix new my-rsmix-app
cd my-rsmix-app
rsmix dev
rsmix assets
rsmix assets inspect /assets/app/actions/public/entry.js
rsmix build
rsmix completion bash >> ~/.bashrc
rsmix doctor
rsmix doctor --fix
rsmix routes
rsmix routes --table
rsmix routes --table --no-headers
rsmix test
rsmix version
rsmix --no-color doctor
```

`rsmix test` passes its arguments to `cargo test`, so `rsmix test frame` runs the tests whose names contain `frame`.

`rsmix routes` and `rsmix assets` ask the app itself (`cargo run --quiet -- routes`), so they always describe the code you are running.

`rsmix version` prints the rsmix release, like `remix version` prints the Remix release. rsmix versions follow the Remix release they port, which is why a weekend project is on 3.1.0.

You can also run the CLI from Rust:

```rust
let code = rsmix_cli::run(&["routes".to_string()], &std::env::current_dir()?);
```

## License

See [LICENSE](https://github.com/todd-skelton/rsmix/blob/main/LICENSE)
