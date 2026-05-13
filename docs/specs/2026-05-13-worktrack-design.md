# Worktrack Design

Date: 2026-05-13

## Goal

Build a Rust terminal app for tracking GitHub issues in one repository at a time. The app should feel like a focused todo list for repository work: show issue state clearly, support filtering, and let the user create issues, comment, close, and reopen issues without opening GitHub in a browser.

The primary example repository is `skunkwork`, but the app should accept any `owner/repo`.

## Stack

- Rust application binary.
- Ratatui for terminal UI rendering.
- TachyonFX for terminal animations and visual transitions.
- Octocrab for GitHub API calls.
- GitHub CLI (`gh`) as the auth source.

The app will read a token from `gh auth token` and pass it to Octocrab. It will not store or manage GitHub credentials.

## Launch And Repository Resolution

The app supports two launch modes:

```bash
worktrack owner/skunkwork
worktrack
```

When an explicit repository is passed, the app validates and stores it as `{ owner, name }`.

When no repository is passed, the app tries to infer the current repository by running:

```bash
gh repo view --json nameWithOwner
```

If repo inference fails, the app shows a blocking setup error with the expected command shape.

## Main Screen

The main screen is a focused issue list with state and filters visible at all times.

```text
owner/skunkwork                                   open: 12  closed: 4  all: 16
[State: open] [Assignee: me] [Labels: bug, ui] [Search: _]

#122  open    bug        Fix login redraw
#119  open    enhancement Add keyboard shortcuts
#101  closed  docs       Clarify setup

──────────────────────────────────────────────────────────────────────────────
Fix login redraw
labels: bug, tui       assignees: kpowel859       updated: 2026-05-12

<issue body and comments preview>

q quit | r refresh | / search | f filters | c comment | n new issue | x close/reopen
```

The layout uses:

- Header with repo name and issue counts.
- Persistent filter bar.
- Issue list with number, state, primary labels, and title.
- Detail pane for the selected issue body and comments preview.
- Footer/status bar for shortcuts, progress, and recoverable errors.

## Core Actions

- `r`: refresh issue list and selected issue comments.
- `/`: edit text search.
- `f`: open filter editor.
- `c`: open comment composer for the selected issue.
- `n`: open new issue modal.
- `x`: close or reopen the selected issue after confirmation.
- `Enter`: focus or expand issue detail.
- `Esc`: leave input/modal modes.
- Arrow keys or `j/k`: move selection.
- `q`: quit when not editing text.

Filters include:

- State: `open`, `closed`, `all`.
- Assignee: `me`, `none`, or username.
- Labels: zero or more labels.
- Search text.

Filters apply after confirmation rather than on every keystroke.

## Animation Design

TachyonFX should add polish without slowing down repeated work:

- List refresh shimmer or subtle sweep while loading.
- Selection transition when moving between issues.
- Modal open and close transitions.
- Success pulse after creating, commenting, closing, or reopening.
- Error/status flash for failed operations.

Animations should respect terminal constraints and remain readable on small screens.

## Application State

Core internal types:

- `Repository`: owner and repo name.
- `IssueSummary`: number, title, state, labels, assignees, author, updated time, comment count.
- `IssueDetail`: summary plus body and loaded comments.
- `IssueComment`: author, body, created time.
- `IssueFilters`: state, assignee, labels, text query.
- `UiMode`: browsing, filter editor, comment composer, new issue modal, confirmation, loading, error.
- `PendingAction`: refresh, create issue, add comment, close issue, reopen issue.

The UI layer should depend on an application state object and command messages. GitHub API details stay behind an adapter boundary.

## GitHub Adapter

The adapter owns all Octocrab usage and exposes app-focused methods:

- `list_issues(repo, filters) -> Vec<IssueSummary>`
- `get_issue(repo, number) -> IssueDetail`
- `list_comments(repo, number) -> Vec<IssueComment>`
- `create_issue(repo, title, body, labels) -> IssueSummary`
- `add_comment(repo, number, body) -> IssueComment`
- `set_issue_state(repo, number, state) -> IssueSummary`

The adapter handles pagination, maps API models into app models, and normalizes errors for UI display. Token values must never be included in errors.

## Error Handling

Blocking setup errors use an overlay:

- `gh` is missing.
- `gh auth token` fails.
- Repo argument is invalid.
- Repo inference fails.

Recoverable runtime errors stay in the status bar:

- Refresh failed.
- Comment submission failed.
- Issue creation failed.
- Close or reopen failed.
- Rate limit or permission errors.

The app should keep the previous successful data visible when refreshes fail.

## Testing

Tests should cover:

- Repo parsing and validation.
- Repo inference failure handling.
- Filter state transitions.
- Keyboard action mapping by UI mode.
- GitHub adapter contract behavior through mocked responses.
- Ratatui screen snapshots for the main list, filter editor, comment composer, new issue modal, and error overlay.

Normal tests should not hit GitHub. An ignored integration test may verify Octocrab behavior against a real repo when a token is available.

## Out Of Scope For V1

- Multi-repo inbox.
- Project board synchronization.
- Pull request review workflows.
- Local persistent database.
- Custom token storage.
- Offline write queue.
