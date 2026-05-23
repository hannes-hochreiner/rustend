# GitHub Actions CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a single GitHub Actions workflow that runs the full rustend test suite on every push and pull request, with `rustend-core` unit tests gating parallel `rustend-server` and `rustend-client` jobs.

**Architecture:** One workflow file (`.github/workflows/ci.yml`) with three jobs. `test-core` runs first; `test-server` and `test-client` both declare `needs: [test-core]` and run concurrently once it passes. All jobs use `Swatinem/rust-cache@v2` for Cargo caching. In `test-client`, Firefox is installed via `browser-actions/setup-firefox@v1`, wasm-pack via `taiki-e/install-action@v2`, and geckodriver via an inline shell step with authenticated GitHub API + `jq` + SHA-256 verification.

**Tech Stack:** GitHub Actions, Rust/Cargo, wasm-pack, Firefox (`browser-actions/setup-firefox@v1`), geckodriver

**Working directory:** Run all commands from the root of the `worktree-rustend-implementation` worktree (that is, the repository root for this checkout).

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
      - name: Install Firefox
        uses: browser-actions/setup-firefox@v1
      - uses: taiki-e/install-action@v2
        with:
          tool: wasm-pack
      - name: Install geckodriver
        run: |
          GECKODRIVER_VERSION=$(curl -sf -H "Authorization: Bearer ${{ secrets.GITHUB_TOKEN }}" \
            https://api.github.com/repos/mozilla/geckodriver/releases/latest \
            | jq -r '.tag_name | ltrimstr("v")')
          [ -n "$GECKODRIVER_VERSION" ] || { echo "Failed to detect geckodriver version"; exit 1; }
          TARBALL="geckodriver-v${GECKODRIVER_VERSION}-linux64.tar.gz"
          curl -sLO "https://github.com/mozilla/geckodriver/releases/download/v${GECKODRIVER_VERSION}/${TARBALL}"
          curl -sLO "https://github.com/mozilla/geckodriver/releases/download/v${GECKODRIVER_VERSION}/${TARBALL}.sha256"
          sha256sum --strict --check "${TARBALL}.sha256"
          tar -xzf "${TARBALL}"
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
