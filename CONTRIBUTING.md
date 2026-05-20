# Contributing to tissues

Thanks for helping improve `tissues`. This project is a Rust terminal app for
working through GitHub issues and GitHub Projects from one repository at a time.

## Ways to contribute

- Report bugs with reproduction steps, terminal size, operating system, and the
  `tissues --version` output.
- Request features by describing the workflow you want to improve.
- Improve documentation when setup, auth, board movement, or contribution steps
  are unclear.
- Send focused pull requests that keep behavior, tests, and docs aligned.

## Development setup

Install the Rust toolchain and GitHub CLI, then clone the repository:

```bash
git clone https://github.com/Kade-Powell/tissues.git
cd tissues
gh auth login
```

Run the checks used by CI:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

For a faster local loop, install and use `bacon`:

```bash
cargo install bacon
bacon
```

Run the interactive TUI in a normal terminal:

```bash
cargo run -- Kade-Powell/tissues
```

For restart-on-change TUI development:

```bash
cargo install watchexec-cli
./scripts/dev-tui Kade-Powell/tissues
```

Do not run the full TUI inside `bacon`; both applications use terminal alternate
screen behavior.

## Git hooks and commits

Install the local hooks before committing:

```bash
python3 -m pip install --user prek
prek install
prek install --hook-type pre-push
```

Commit messages must use Conventional Commits:

```text
feat(tui): add board picker
fix(auth): retry scope repair
docs: clarify project setup
```

Conventional Commit types drive the release workflow:

- `feat:` creates a minor release.
- `fix:`, `docs:`, `ci:`, and other supported types create a patch release.
- `!` or `BREAKING CHANGE:` creates a major release.

## Pull request checklist

Before opening a pull request:

- Keep the change focused on one problem or workflow.
- Update `README.md`, `docs/USER_GUIDE.md`, or templates when user behavior
  changes.
- Add or update tests for behavior changes when practical.
- Run `cargo fmt --check`.
- Run `cargo test`.
- Use a Conventional Commit subject.

## TUI architecture

The terminal UI is built around `tui-realm`.

- Route terminal input through `tuirealm::Application::tick` and component
  `AppComponent::on` handlers.
- Represent UI events as application messages and apply state changes in the
  model/update layer.
- Mount visible UI areas as `tui-realm` components with stable component IDs.
- Keep direct Ratatui rendering inside `tui-realm` component `view` methods or
  helpers called by those methods.
- Keep GitHub API calls and other side effects out of component render methods.
- Do not add direct `crossterm::event::poll` or `crossterm::event::read` loops.
- Do not add top-level screen render functions that bypass mounted
  `tui-realm` components.

## Security

Do not open public issues for suspected vulnerabilities or token leaks. Follow
the reporting process in [SECURITY.md](SECURITY.md).
