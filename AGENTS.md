# Agent Instructions

This repository is migrating its terminal UI to `tui-realm`. Agents working on TUI code must preserve that direction.

## Required TUI Architecture

- Use `tuirealm` as the application framework for terminal UI control flow.
- Route terminal input through `tuirealm::Application::tick` and component `AppComponent::on` handlers.
- Represent UI events as application messages and apply state changes in a model/update layer.
- Mount visible UI areas as `tui-realm` components with stable component IDs.
- Prefer `tui-realm-stdlib`, `tui-realm-textarea`, and `tui-realm-treeview` components before adding custom Ratatui widgets.
- Keep direct Ratatui rendering inside `tui-realm` component `view` methods only.
- Keep GitHub API calls and other side effects out of component render methods.

## Prohibited Patterns

- Do not add new direct `crossterm::event::poll` or `crossterm::event::read` loops in application code.
- Do not add new top-level screen render functions that bypass mounted `tui-realm` components.
- Do not put backend calls in component `view` or `on` methods.
- Do not introduce a second TUI framework or a parallel input dispatcher.

## Testing Expectations

- Add or update tests before behavior changes when practical.
- Keep navigation tests around issue list browsing, Enter-to-detail, Escape-to-list, comments, filters, and editor flows.
- Run `cargo fmt --check` and `cargo test` before claiming the migration work is complete.
