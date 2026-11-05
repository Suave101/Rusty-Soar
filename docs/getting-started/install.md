# 1. Install

## Install Rust

Rusty-Soar is built with Rust's standard tool, `cargo`. If you don't have Rust yet, install it
with [rustup](https://rustup.rs/):

=== "Linux and macOS"

    ```bash
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

=== "Windows"

    Download and run `rustup-init.exe` from [rustup.rs](https://rustup.rs/).

When it finishes, open a **new** terminal and check that it worked:

```bash
cargo --version
```

You should see a version number, such as `cargo 1.90.0`.

## Get the code

```bash
git clone https://github.com/Suave101/Rusty-Soar.git
cd Rusty-Soar/rusty-soar
```

!!! note

    The Rust code lives in the `rusty-soar` folder inside the repository. Every `cargo`
    command in this documentation is run from that folder unless it says otherwise.

## Build and test

```bash
cargo build
cargo test --workspace
```

The first build downloads a few libraries and takes a minute or two. The tests should all pass;
every line of the output that starts with `test result:` should say `ok`.

!!! failure "Something went wrong?"

    - **`cargo: command not found`**: open a new terminal so it picks up the Rust install.
    - **Errors while compiling**: run `rustup update` to get the latest stable Rust, then try
      again.

## What's in the repository

| Folder | What it holds |
|---|---|
| `rusty-soar/crates/rusty-soar-core` | The Soar kernel itself, as a Rust library |
| `rusty-soar/crates/rusty-soar-cli` | A command-line tool for running agents |
| `rusty-soar/crates/formal-soar-verify` | Formal proofs, checked with Kani |
| `differential/` | Tools for comparing Rusty-Soar with C++ Soar |
| `docs/` | This documentation |

**Next:** [2. Run Your First Agent](first-agent.md)
