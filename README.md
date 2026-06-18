# tissues

tissues is a Rust terminal app for working through GitHub issues in one repository at a time. It uses Ratatui for the interface, TachyonFX for terminal animations, Octocrab for GitHub API calls, and the GitHub CLI for authentication.

It is built for maintainers who want a fast issue list, a GitHub Projects-style
board, inline editing, labels, assignees, comments, and auth diagnostics without
leaving the terminal.

For setup, workflows, board movement, auth scopes, and troubleshooting, see the
[user guide](docs/USER_GUIDE.md).

## Features

- Browse open, closed, or all issues for a single repository.
- Toggle between the issue list and a GitHub Projects board view.
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
- Animate newly arrived and mentioned issues in the list without adding text badges.
- Show issue author, assignees, relative age, mentions, and stale state in the list.
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

For a checkout that should use a specific GitHub CLI account, create
`.tissues/config.json` in that checkout:

```json
{
  "auth": {
    "gh_user": "Kade-Powell"
  }
}
```

When configured, tissues reads the token with `gh auth token --user Kade-Powell`.
If an auth repair needs new scopes, tissues switches `gh` to that account before
running `gh auth refresh`. The `.tissues` directory is ignored by git so the
account choice stays local to the checkout.

Creating issues requires a token that can write issues in the repository. For
GitHub CLI OAuth tokens, refresh repository access with:

```bash
gh auth refresh -s repo
```

Loading GitHub Projects requires read access to Projects:

```bash
gh auth refresh -s read:project
```

Moving issues through GitHub Project board states requires project write access:

```bash
gh auth refresh -s project
```

## Run

Install from crates.io:

```bash
cargo install tissues --locked
```

Validate:

```bash
tissues --version
tissues --help
```

Open a specific repository:

```bash
tissues owner/tissues
```

GitHub remote URLs work too:

```bash
tissues git@github.com:Kade-Powell/tissues.git
```

Or run from source:

```bash
cargo run -- owner/tissues
```

Or run inside a GitHub checkout and let `gh repo view` infer the repository:

```bash
cargo run
```

## Controls

- `j` / `Down`: move to the next issue.
- `k` / `Up`: move to the previous issue.
- `v`: toggle between list and board view.
- `:move <state>`: move the selected issue to a GitHub Project board state while in board view. Use `:move` to list available states.
- `:tree`: render the current list or board as an issue relationship tree. Use `:tree off` to return to flat rows.
- `:branch`: create and switch to a local git branch for the selected issue, then show the `Closes #123` PR body line GitHub needs to close it on merge.
- `Enter`: collapse or expand comments in the detail tree.
- `:`: open command mode at the bottom of the screen.
- `Tab`: complete the highlighted command suggestion while in command mode.
- `Space`: toggle users while assigning, then `Enter` submits all selected assignees.
- `n`: create a new issue with separate title, Markdown body, and labels fields.
- `x`: close the selected open issue with a required comment, or reopen a closed issue after confirmation.
- `d`: in triage mode, open a confirmation modal to permanently delete the selected issue.
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
- `:doctor`: check GitHub auth, issue access, and project board readiness.
- `:board`: open the board view.
- `:boards`: choose from repository GitHub Projects.
- `:tree`: render the current issue list or board columns as a relationship tree.
- `:branch`: create and switch to a local branch named from the selected issue. Add the shown `Closes #123` line to the PR body so GitHub closes the issue when the PR merges.
- `:list`: return to the issue list.
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

While browsing, tissues automatically reloads issues every 5 seconds. If the refreshed list contains issue numbers that were not already visible, or new comments, new issue descriptions, or updated issue bodies mention your authenticated GitHub username, the footer shows the notification, the terminal bell rings, and macOS plays the system notification sound when available.

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
cargo run -- owner/tissues
```

For restart-on-change development of the actual TUI, install `watchexec` and run the helper script in a normal terminal:

```bash
cargo install watchexec-cli
./scripts/dev-tui owner/tissues
```

Do not run the full tissues TUI as a bacon job. Bacon is also a terminal UI, and nesting tissues inside it can leave the terminal alternate screen, mouse capture, or formatting in a bad state. Bacon's `run` and `run-long` jobs are intentionally overridden to print a reminder instead of launching tissues. The included `smoke` job intentionally runs only `cargo run -- --help`.

Run the checks used for this project:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Install the local git hooks with `prek` before committing:

```bash
prek install
prek install --hook-type pre-push
```

The commit-message hook enforces Conventional Commits, and the pre-push hook runs the quick formatting and packaging checks. Conventional Commit messages also drive the release workflow.

Releases are driven by Conventional Commit messages on `main`. Breaking changes create a major release, `feat:` creates a minor release, and any other Conventional Commit type creates a patch release. The release workflow commits the package version, tags `vX.Y.Z`, creates the GitHub release, and publishes `tissues` to crates.io with `CRATES_IO_TOKEN`.

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the
development loop, commit conventions, test expectations, and TUI architecture
rules. Please report security issues through [SECURITY.md](SECURITY.md) rather
than public issues.

The core code is split by responsibility:

- `src/app.rs`: UI state, filters, selection, and modes.
- `src/cache.rs`: local issue list and detail cache.
- `src/config.rs`: built-in and user-defined saved views.
- `src/domain.rs`: issue, comment, label, and user models.
- `src/github.rs`: Octocrab adapter and `gh auth token` integration.
- `src/repo.rs`: repository parsing and `gh repo view` inference.
- `src/realm.rs`: mounted tui-realm components and input translation.
- `src/tui.rs`: model updates and live issue operations.
- `src/ui.rs`: component rendering helpers and TachyonFX effects.

## GitHub Projects

The board view loads a GitHub Projects board when one is available. If multiple
repository projects are available, tissues shows a board picker. To pin a user
or organization project, add `project_board` to `~/.config/tissues/config.json`:

```json
{
  "project_board": {
    "owner": "Kade-Powell",
    "number": 1,
    "status_field": "Status"
  }
}
```

The `owner` is the user or organization that owns the project, `number` is the
project number from GitHub, and `status_field` defaults to `Status`.

## Current Scope

This is a v1 focused issue tracker. It intentionally does not include multi-repo inboxes, pull request review workflows, or custom token storage.
