# skunkwork

skunkwork is a Rust terminal app for working through GitHub issues in one repository at a time. It uses Ratatui for the interface, TachyonFX for terminal animations, Octocrab for GitHub API calls, and the GitHub CLI for authentication.

## Features

- Browse open, closed, or all issues for a single repository.
- Filter by issue state, assignee, labels, and search text.
- Sort by updated time, created time, comment count, or assignee.
- View the selected issue body and comments in a collapsible tree.
- Render issue descriptions and comments as Markdown in the detail pane.
- Create new issues, optionally starting from Markdown issue templates in `.github/ISSUE_TEMPLATE`.
- Edit issue title and body inline.
- Add comments to existing issues.
- Complete `@username` mentions from repository collaborators while writing comments and issue bodies.
- Assign issues to yourself, a collaborator, or nobody.
- Edit labels on existing issues.
- Use mouse clicks for issue selection, modal fields, picker rows, and action buttons.
- Close issues with a required closing comment.
- Reopen issues after confirmation.
- Refresh issue data without leaving the terminal.
- Auto-refresh issue data every 5 seconds while browsing.
- Show a visible notification, terminal bell, and macOS system sound when new issues arrive.
- Notify when new comments mention your authenticated GitHub username.
- Notify when new issue descriptions or updated issue bodies mention your authenticated GitHub username.
- Animate newly arrived issues in the list with a temporary `NEW` row highlight.
- Animate mentioned issues in the list with a temporary `PING` row highlight.
- Show issue author, assignees, relative age, mention badges, and stale badges in the list.
- Show a confirmation after successful writes once issue state has reloaded.
- Show compact loading indicators for routine actions with contained TachyonFX movement.
- Use a simple TachyonFX coalesce effect for loading and completion feedback.
- Use exabind-inspired frame glyphs while inheriting the user's terminal theme.
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
- `:`: open command mode at the bottom of the screen.
- `Tab`: complete the highlighted command suggestion while in command mode.
- `Space`: toggle users while assigning, then `Enter` submits all selected assignees.
- `n`: create a new issue with separate title, Markdown body, and labels fields.
- `x`: close the selected open issue with a required comment, or reopen a closed issue after confirmation.
- `Tab` / `Shift+Tab`: move between new issue fields.
- `Tab`: complete an active `@username` mention while writing a comment or issue body.
- Arrow keys, `Home`, `End`, `Backspace`, and `Delete`: edit text at the cursor in command, search, comment, issue, and picker fields.
- `Enter`: move from title to body, insert body newlines, or add the current label.
- `Ctrl+T`: apply the next available issue template while creating a new issue.
- `Ctrl+S`: submit a comment, close with comment, or create the issue from any new issue field.
- `Ctrl+Enter` or `Ctrl+J`: insert a newline while writing an issue body or comment.
- `Esc`: cancel the current input or modal.
- Mouse: click issue rows, detail tree, picker rows, text fields, and action buttons.
- `q`: quit when not editing text.

Useful commands:

- `:refresh`: reload issues now.
- `:all`, `:clear`, or `:clear filters`: clear state, assignee, label, and search filters.
- `:fs` or `:filter state`: cycle state filter: open, closed, all.
- `:fa` or `:filter assignee`: choose an assignee filter: any, me, unassigned, or a collaborator.
- `:s <text>` or `:search <text>`: search issue titles.
- `:me`: filter to issues assigned to you.
- `:unassigned`: filter to unassigned issues.
- `:label <name>`: filter to a label. Use `:label any` to clear label filters.
- `:sort`, `:sort updated`, `:sort created`, `:sort comments`, or `:sort assignee`: change issue ordering.
- `:assign`: assign the selected issue to yourself, nobody, or collaborators.
- `:labels`: edit labels on the selected issue.
- `:edit`: edit the selected issue title and Markdown body.
- `:comment`: comment on the selected issue.
- `:ping` or `:mentions`: jump to the first highlighted mention.
- `:new`: create a new issue.
- `:close`: close or reopen the selected issue.
- `:quit`: quit.

While browsing, skunkwork automatically reloads issues every 5 seconds. If the refreshed list contains issue numbers that were not already visible, or new comments, new issue descriptions, or updated issue bodies mention your authenticated GitHub username, the footer shows the notification, the terminal bell rings, and macOS plays the system notification sound when available.

## Development

For the fastest edit/check loop, run bacon from the repo root:

```bash
bacon
```

This watches the project and runs `cargo check --all-targets` after changes. Inside bacon:

- `t`: run tests.
- `u`: run library unit tests.
- `c`: run clippy with the same warning policy used for verification.
- `f`: run `cargo fmt --check`.
- `d`: build docs without dependencies.
- `s`: run the non-interactive help smoke test.

Run the interactive TUI in a separate terminal:

```bash
cargo run -- owner/skunkwork
```

For restart-on-change development of the actual TUI, install `watchexec` and run the helper script in a normal terminal:

```bash
cargo install watchexec-cli
./scripts/dev-tui owner/skunkwork
```

Do not run the full skunkwork TUI as a bacon job. Bacon is also a terminal UI, and nesting skunkwork inside it can leave the terminal alternate screen, mouse capture, or formatting in a bad state. Bacon's `run` and `run-long` jobs are intentionally overridden to print a reminder instead of launching skunkwork. The included `smoke` job intentionally runs only `cargo run -- --help`.

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
