# cargo-pvm-contract

A cargo subcommand to build Rust contracts to PolkaVM bytecode.

This tool is designed for building smart contracts in Rust using the low-level API provided by [pallet-revive-uapi](https://docs.rs/pallet-revive-uapi/latest/pallet_revive_uapi/). For a more high-level, user-friendly API, see [Ink!](https://use.ink/).

To learn more, visit the [Rust Contract Template](https://github.com/paritytech/rust-contract-template).

## Installation

```bash
cargo install --force --locked cargo-pvm-contract
```

## Usage

Once installed, you can use it as a cargo subcommand:

```bash
cargo pvm-contract
```

This launches an interactive prompt to initialize a new contract project.

Examples:

Initialize a new project and build it:

```bash
cargo pvm-contract
cd my_contract
cargo build
```

The PolkaVM bytecode will be written to `target/pvm/<bin>.polkavm` via the generated `build.rs`.


