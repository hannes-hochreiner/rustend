# GitHub Actions CI — Design Specification

**Date:** 2026-05-18
**Status:** Draft
**Scope:** `.github/workflows/ci.yml`

---

## 1. Purpose

Run the full rustend test suite automatically on every push to any branch. Provide fast feedback by running the `rustend-core` unit tests first, then running the server and client integration tests in parallel once core passes.

---

## 2. File Layout

One new file:

```
.github/
└── workflows/
    └── ci.yml
```

---

## 3. Trigger

```yaml
on: push
```

Fires on every push to every branch. Pull-request runs are out of scope for this spec.

---

## 4. Job Structure

Three jobs, all on `ubuntu-latest`:

```
test-core
    │
    ├── test-server
    └── test-client
```

`test-server` and `test-client` each declare `needs: [test-core]`. If `test-core` fails, neither downstream job starts and CI minutes are not consumed.

---

## 5. Job Details

### 5.1 `test-core`

Runs the unit tests for `rustend-core`. No external dependencies.

Steps:
1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable`
3. `Swatinem/rust-cache@v2`
4. `cargo test -p rustend-core`

### 5.2 `test-server`

Runs the integration tests for `rustend-server`. The tests use the `testcontainers` crate, which starts a PostgreSQL Docker container at runtime. The `ubuntu-latest` runner has the Docker daemon running by default — no extra setup step is needed.

Steps:
1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable`
3. `Swatinem/rust-cache@v2`
4. `cargo test -p rustend-server`

### 5.3 `test-client`

Runs the WASM browser tests for `rustend-client` using `wasm-pack` in headless Firefox mode. Firefox is pre-installed on `ubuntu-latest`. `wasm-pack` and `geckodriver` are not, so they are installed as explicit steps.

Steps:
1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable` with `targets: wasm32-unknown-unknown`
3. `Swatinem/rust-cache@v2`
4. Install `wasm-pack` via the official installer script
5. Install `geckodriver` by downloading the latest release binary from GitHub Releases
6. `wasm-pack test --firefox --headless rustend-client`

---

## 6. Caching Strategy

`Swatinem/rust-cache@v2` is used in every job. It caches:

- `~/.cargo/registry` — downloaded crate sources
- `~/.cargo/git` — git-sourced crate checkouts
- `target/` — compiled artifacts

The cache key is derived from the OS, Rust toolchain version, and the hash of `Cargo.lock`. A cache hit on a subsequent push with no dependency changes saves 1–2 minutes of compile time per job.

---

## 7. External Actions Used

| Action | Version | Purpose |
|---|---|---|
| `actions/checkout` | `v4` | Check out the repository |
| `dtolnay/rust-toolchain` | `stable` | Install Rust stable toolchain |
| `Swatinem/rust-cache` | `v2` | Cache Cargo registry and build artifacts |

`wasm-pack` and `geckodriver` are installed via inline shell steps rather than third-party actions to avoid pinning to additional external action repositories.

---

## 8. Out of Scope

- Pull-request triggers
- Scheduled runs
- Publishing or releasing artifacts
- Notification on failure (e.g. Slack, email)
