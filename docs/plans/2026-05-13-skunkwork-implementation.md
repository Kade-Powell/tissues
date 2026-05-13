# skunkwork Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a working Rust TUI for browsing, filtering, creating, commenting on, closing, and reopening GitHub issues for one repository at a time.

**Architecture:** The binary separates domain state, GitHub API access, terminal rendering, and event handling. The UI uses Ratatui and TachyonFX, while GitHub operations use Octocrab authenticated by `gh auth token`.

**Tech Stack:** Rust, Tokio, Clap, Ratatui, Crossterm, TachyonFX, Octocrab, Serde, Color-eyre.

---

### Task 1: Scaffold Rust Project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`

- [ ] **Step 1: Initialize package metadata and dependencies**

Create a Rust binary package named `skunkwork` with dependencies for async GitHub access, TUI rendering, terminal input, command-line parsing, and tests.

- [ ] **Step 2: Run baseline build**

Run: `cargo test`

Expected: the project compiles and the empty test suite passes.

### Task 2: Domain Model And Repo Resolution

**Files:**
- Create: `src/domain.rs`
- Create: `src/repo.rs`
- Test: `src/repo.rs`

- [ ] **Step 1: Write failing repo parsing tests**

Cover valid `owner/repo`, invalid missing owner, invalid missing repo, and invalid extra slash.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test repo::tests`

Expected: compile failure or failing tests because the parser does not exist yet.

- [ ] **Step 3: Implement `Repository` parsing and display**

Implement a strict `Repository { owner, name }` parser and `Display`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test repo::tests`

Expected: all repo tests pass.

### Task 3: App State And Filtering

**Files:**
- Create: `src/app.rs`
- Test: `src/app.rs`

- [ ] **Step 1: Write failing state transition tests**

Cover default filters, state cycling, issue selection clamping, and query updates.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test app::tests`

Expected: compile failure or failing tests because app state does not exist yet.

- [ ] **Step 3: Implement app state**

Create `App`, `IssueFilters`, `IssueState`, `UiMode`, and selection helpers.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test app::tests`

Expected: all app state tests pass.

### Task 4: GitHub Adapter

**Files:**
- Create: `src/github.rs`
- Modify: `src/domain.rs`

- [ ] **Step 1: Define adapter boundary**

Create a `GitHubClient` wrapper that constructs Octocrab from a token and exposes list, create, comment, close, and reopen methods.

- [ ] **Step 2: Add token loader**

Implement `load_gh_token()` using `gh auth token`, with token text trimmed and never included in error strings.

- [ ] **Step 3: Build check**

Run: `cargo test`

Expected: build succeeds.

### Task 5: Terminal UI And Animations

**Files:**
- Create: `src/ui.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add render tests**

Render the main screen with sample issues into a Ratatui test backend and assert the buffer contains repo name, filters, issue state, and footer actions.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test ui::tests`

Expected: compile failure or failing tests because rendering does not exist yet.

- [ ] **Step 3: Implement rendering**

Render header, filter bar, issue list, detail pane, status/footer, and modal overlays.

- [ ] **Step 4: Add TachyonFX usage**

Create a small effect timeline for refresh and success pulses, and wire it into the draw path without making the app depend on animation for readability.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test ui::tests`

Expected: all UI rendering tests pass.

### Task 6: Event Loop And Live Operations

**Files:**
- Modify: `src/main.rs`
- Create: `src/tui.rs`

- [ ] **Step 1: Wire CLI arguments**

Accept optional `owner/repo`; infer via `gh repo view --json nameWithOwner` when omitted.

- [ ] **Step 2: Wire terminal loop**

Enter alternate screen, load issues, handle keys, draw UI, and restore terminal on exit.

- [ ] **Step 3: Wire issue actions**

Implement refresh, filters, comment composer, new issue modal, and close/reopen confirmation.

- [ ] **Step 4: Run full verification**

Run: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test`.

Expected: all commands succeed.
