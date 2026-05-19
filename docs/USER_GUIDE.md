# tissues user guide

`tissues` is a terminal app for working through GitHub issues in one repository
at a time. It uses your existing GitHub CLI login, so you do not need to paste a
token into the app.

## Install

Install from crates.io:

```bash
cargo install tissues --locked
```

Check that the binary is available:

```bash
tissues --version
tissues --help
```

You also need the GitHub CLI:

```bash
gh auth login
```

## Open a repository

Open a repository by owner and name:

```bash
tissues Kade-Powell/tissues
```

GitHub remote URLs also work:

```bash
tissues git@github.com:Kade-Powell/tissues.git
```

If you run `tissues` inside a GitHub checkout, it can infer the repository from
the local git remote through `gh repo view`.

## Authentication

By default, `tissues` reads the active GitHub CLI token with:

```bash
gh auth token
```

For normal issue browsing, your account needs repository read access. Creating,
editing, assigning, labeling, commenting on, closing, or reopening issues
requires repository write access. With GitHub CLI OAuth tokens, refresh the repo
scope with:

```bash
gh auth refresh -s repo
```

Loading GitHub Projects requires project read access:

```bash
gh auth refresh -s read:project
```

Moving issues through GitHub Project board states requires project write access:

```bash
gh auth refresh -s project
```

### Use a specific GitHub account for one checkout

If your global active `gh` account is not the account you want for this
repository, create `.tissues/config.json` in the checkout:

```json
{
  "auth": {
    "gh_user": "Kade-Powell"
  }
}
```

Then `tissues` reads:

```bash
gh auth token --user Kade-Powell
```

The `.tissues` directory is ignored by git, so this setting stays local to your
machine.

## Layout

The smallest terminal width uses a plain issue list. Wider terminals add richer
context: tabs, filters, board columns, summary panels, author, assignee, age,
stale state, and mention indicators.

The footer always shows the commands that matter for the current screen. If a
key is needed for the current mode, it should be visible there.

## Basic navigation

- `j` or `Down`: select the next issue.
- `k` or `Up`: select the previous issue.
- `Enter`: open the selected issue detail. In detail view, it folds or unfolds
  comments.
- `Esc`: close the current modal or return from detail to the issue list.
- `v`: toggle between list and board view.
- `:`: open command mode.
- `q`: quit when you are not editing text.

Mouse selection also works for issue rows, picker rows, modal fields, and action
buttons.

## List view

List view is the fastest way to work through a filtered set of issues. Use it
for triage, search, quick edits, comments, and issue creation.

Useful commands:

- `:refresh`: reload issues.
- `:all`, `:clear`, or `:clear filters`: clear filters.
- `:fs` or `:filter state`: cycle open, closed, and all issues.
- `:fa` or `:filter assignee`: choose an assignee filter.
- `:s <text>` or `:search <text>`: search issue titles.
- `:me`: show issues assigned to you.
- `:unassigned`: show unassigned issues.
- `:label <name>`: filter to a label.
- `:label any`: clear label filters.
- `:sort updated`, `:sort created`, `:sort comments`, or `:sort assignee`:
  change issue ordering.

## Board view

Board view mirrors a GitHub Projects board when the repository has one attached.
Open it with:

```text
:board
```

If the repository has multiple project boards, `tissues` shows a picker. You can
also open that picker directly:

```text
:boards
```

Use:

- `j` / `k`: select issues inside the board.
- `Left` / `Right`: move the selected issue to the previous or next GitHub
  Project board state.
- `Enter`: open the selected issue detail.
- `v`: return to list view.

Board movement updates GitHub Projects directly, then reloads the board so the
terminal view matches GitHub.

### Pin a board

To always open a specific user or organization project, add this to
`~/.config/tissues/config.json`:

```json
{
  "project_board": {
    "owner": "Kade-Powell",
    "number": 5,
    "status_field": "Status"
  }
}
```

`owner` is the user or organization that owns the project. `number` is the
GitHub Project number. `status_field` defaults to `Status`.

## Create and edit issues

- `n` or `:new`: create an issue.
- `Ctrl+T`: apply the next issue template while creating an issue.
- `Ctrl+S`: submit a new issue, comment, close comment, or edit.
- `:edit`: edit the selected issue title and body.
- `:comment`: comment on the selected issue.
- `:close`: close an open issue with a required comment, or reopen a closed
  issue after confirmation.

Text fields support arrow keys, `Home`, `End`, `Backspace`, and `Delete`.
`Ctrl+Enter` or `Ctrl+J` inserts a newline while writing an issue body or
comment.

## Assign and label issues

- `:assign`: assign the selected issue to yourself, nobody, or collaborators.
- `Space`: toggle users in the assignee picker.
- `:labels`: edit labels on the selected issue.
- `Enter`: toggle a label in the label picker.
- `Ctrl+S`: save label changes.

While writing comments or issue bodies, `Tab` completes active `@username`
mentions from repository collaborators.

## Triage mode

Press `t` to enter triage mode. Triage mode gives quick single-key actions for
the selected issue:

- `a`: assign the issue to yourself.
- `l`: edit labels.
- `c`: comment.
- `x`: close or reopen.
- `s`: skip to the next issue.
- `t`: exit triage mode.

## Notifications and refresh

While browsing, `tissues` automatically reloads issues every 5 seconds. It keeps
the refresh effect contained to the issue area and does not replay the startup
animation.

When new issues arrive, or new comments/descriptions mention your authenticated
GitHub username, the footer shows a notification and the terminal bell rings.
On macOS, the system notification sound plays when available.

## Error details

When GitHub rejects an operation, `tissues` shows a standard error modal with:

- what failed,
- the detailed GitHub or GraphQL error,
- a suggested next command when one is known.

If the error is caused by a missing GitHub CLI scope, the modal shows a repair
action. Press `r` to run the matching `gh auth refresh -s ...` command, then
retry the failed action.

Press `Esc` to dismiss the error and return to the previous workflow.

## Troubleshooting

Check the active GitHub CLI accounts and scopes:

```bash
gh auth status
```

If issue writes fail, refresh repository access:

```bash
gh auth refresh -s repo
```

If project boards fail to load, refresh project read access:

```bash
gh auth refresh -s read:project
```

If moving board items fails, refresh project write access:

```bash
gh auth refresh -s project
```

If the wrong GitHub account is being used for this checkout, add
`.tissues/config.json` with the `auth.gh_user` setting shown above.
