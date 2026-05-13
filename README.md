# Worktrack

Worktrack is a Rust terminal app for working through GitHub issues in one repository at a time. It uses Ratatui for the interface, TachyonFX for terminal animations, Octocrab for GitHub API calls, and the GitHub CLI for authentication.

## Features

- Browse open, closed, or all issues for a single repository.
- Filter by issue state and search text.
- View the selected issue body and comments in a collapsible tree.
- Render issue descriptions and comments as Markdown in the detail pane.
- Create new issues.
- Add comments to existing issues.
- Close issues with a required closing comment.
- Reopen issues after confirmation.
- Refresh issue data without leaving the terminal.
- Auto-refresh issue data every 10 seconds while browsing.
- Show a visible notification, terminal bell, and macOS system sound when new issues arrive.
- Show a confirmation after successful writes once issue state has reloaded.
- Show compact loading indicators for routine actions.
- Use TachyonFX for the initial startup load plus success and error states.
- Use your existing `gh` login instead of storing a token in the app.

## Requirements

- Rust toolchain.
- GitHub CLI installed as `gh`.
- Authenticated GitHub CLI session:

```bash
gh auth login
```

The app reads a token from:

```bash
gh auth token
```

It does not persist GitHub credentials.

## Run

Open a specific repository:

```bash
cargo run -- owner/skunkwork
```

Or run inside a GitHub checkout and let `gh repo view` infer the repository:

```bash
cargo run
```

## Controls

- `j` / `Down`: move to the next issue.
- `k` / `Up`: move to the previous issue.
- `Enter`: collapse or expand comments in the detail tree.
- `r`: refresh issues.
- `/`: edit search text.
- `f`: cycle state filter: open, closed, all.
- `c`: comment on the selected issue.
- `n`: create a new issue with separate title, Markdown body, and labels fields.
- `x`: close the selected open issue with a required comment, or reopen a closed issue after confirmation.
- `Tab` / `Shift+Tab`: move between new issue fields.
- `Enter`: move from title to body, insert body newlines, or add the current label.
- `Ctrl+S`: submit a comment, close with comment, or create the issue from any new issue field.
- `Ctrl+Enter` or `Ctrl+J`: insert a newline while writing an issue body or comment.
- `Esc`: cancel the current input or modal.
- `q`: quit when not editing text.

While browsing, Worktrack automatically reloads issues every 10 seconds. If the refreshed list contains issue numbers that were not already visible, the footer shows the new issue, the terminal bell rings, and macOS plays the system notification sound when available.

## Development

Run the checks used for this project:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The core code is split by responsibility:

- `src/app.rs`: UI state, filters, selection, and modes.
- `src/domain.rs`: issue, comment, label, and user models.
- `src/github.rs`: Octocrab adapter and `gh auth token` integration.
- `src/repo.rs`: repository parsing and `gh repo view` inference.
- `src/tui.rs`: terminal event loop and live issue operations.
- `src/ui.rs`: Ratatui rendering and TachyonFX effects.

## Current Scope

This is a v1 focused issue tracker. It intentionally does not include multi-repo inboxes, project board sync, pull request review workflows, offline write queues, or custom token storage.
