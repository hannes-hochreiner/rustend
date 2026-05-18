# GitHub Actions CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a single GitHub Actions workflow that runs the full rustend test suite on every push and pull request, with `rustend-core` unit tests gating parallel `rustend-server` and `rustend-client` jobs.

**Architecture:** One workflow file (`.github/workflows/ci.yml`) with three jobs. `test-core` runs first; `test-server` and `test-client` both declare `needs: [test-core]` and run concurrently once it passes. All jobs use `Swatinem/rust-cache@v2` for Cargo caching. `wasm-pack` and `geckodriver` are installed via inline shell steps in the `test-client` job.

**Tech Stack:** GitHub Actions, Rust/Cargo, wasm-pack, Firefox (pre-installed on `ubuntu-latest`), geckodriver

**Working directory:** Run all commands from the root of the `worktree-rustend-implementation` worktree (`/home/hannes/Repository/rustend/.claude/worktrees/rustend-implementation/`).

---

### Task 1: Create the workflow file

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create the workflows directory**

```bash
mkdir -p .github/workflows
```

- [ ] **Step 2: Write `.github/workflows/ci.yml`**

Create `.github/workflows/ci.yml` with this exact content:

```yaml
name: CI

on:
  push:
  pull_request:

jobs:
  test-core:
    name: Test rustend-core
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run unit tests
        run: cargo test -p rustend-core

  test-server:
    name: Test rustend-server
    runs-on: ubuntu-latest
    needs: [test-core]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run integration tests
        run: cargo test -p rustend-server

  test-client:
    name: Test rustend-client
    runs-on: ubuntu-latest
    needs: [test-core]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
      - uses: Swatinem/rust-cache@v2
      - name: Install wasm-pack
        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
      - name: Install geckodriver
        run: |
          GECKODRIVER_VERSION=$(curl -s https://api.github.com/repos/mozilla/geckodriver/releases/latest \
            | grep '"tag_name"' | head -1 | sed 's/.*"v\([^"]*\)".*/\1/')
          curl -sL "https://github.com/mozilla/geckodriver/releases/download/v${GECKODRIVER_VERSION}/geckodriver-v${GECKODRIVER_VERSION}-linux64.tar.gz" \
            | tar -xz
          sudo mv geckodriver /usr/local/bin/
      - name: Run WASM browser tests
        run: wasm-pack test --firefox --headless rustend-client
```

- [ ] **Step 3: Validate YAML syntax**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml')); print('YAML syntax OK')"
```

Expected output:
```
YAML syntax OK
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add GitHub Actions workflow for push and pull request"
```
