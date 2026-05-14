# tui-realm Migration Plan

> **For:** Codex agents working on `tissue`
> **Goal:** Fully migrate the terminal UI control flow and screen structure to `tui-realm`.

## Requirements

- Use `tuirealm` as the runtime framework.
- Use mounted components and application messages for TUI behavior.
- Preserve the current issue-list workflow:
  - list browsing does not fetch detail data,
  - Enter opens issue detail,
  - Escape returns to the list,
  - detail pane uses slide-in and slide-out TachyonFX animations,
  - comments wrap within the visible detail pane.
- Preserve existing issue commands, filters, editors, comment composer, label picker, and assignee picker.
- Reduce unnecessary GitHub calls by keeping detail fetches explicit.

## Current Shape

- `src/tui.rs` owns the main loop, direct Crossterm input polling, key dispatch, mouse dispatch, and async side effects.
- `src/ui.rs` owns most rendering as direct Ratatui functions.
- The app state already functions like an Elm model in `src/app.rs`, but messages and mounted components do not exist yet.

## Target Shape

- `src/realm.rs`: owns `tuirealm::Application`, component IDs, framework setup, event polling, and message dispatch.
- `src/message.rs`: contains UI messages emitted by components.
- `src/components/`: contains mounted `tui-realm` components for header, filters, issue list, detail pane, footer/status, and modal/editor surfaces.
- `src/tui.rs`: becomes the async update/effects layer that handles GitHub calls in response to messages.
- `src/ui.rs`: shrinks to reusable drawing helpers used by component `view` methods.

## Implementation Steps

1. Add `tuirealm`, `tui-realm-stdlib`, `tui-realm-textarea`, and `tui-realm-treeview` dependencies.
2. Add `RealmId` and `RealmMsg` types.
3. Add a first mounted root component so the runtime is controlled by `tuirealm::Application`.
4. Replace the direct `crossterm::event::poll/read` loop with `Application::tick`.
5. Translate `RealmMsg` into the existing async update functions to preserve behavior while moving the loop.
6. Split root rendering into mounted components:
   - issue list,
   - issue detail,
   - footer/status,
   - command/search input,
   - new issue editor,
   - issue editor,
   - assignee picker,
   - label picker,
   - confirmation/success/error surfaces.
7. Move keyboard and mouse handling from central `match app.mode` dispatch into the responsible component `AppComponent::on` handlers.
8. Replace direct Ratatui text-entry widgets with `tui-realm-textarea` where it fits existing multiline editor behavior.
9. Replace custom picker/list widgets with `tui-realm-stdlib` or `tui-realm-treeview` components where practical.
10. Remove transitional root rendering once all visible surfaces are mounted components.
11. Verify with `cargo fmt --check` and `cargo test`.

## Checkpoints

- Checkpoint 1: App compiles with `tui-realm` dependencies and an `AGENTS.md` guardrail.
- Checkpoint 2: Main loop input polling is handled by `Application::tick`.
- Checkpoint 3: Issue list and issue detail are separate mounted components.
- Checkpoint 4: Editors and pickers are mounted components.
- Checkpoint 5: Transitional direct screen rendering is removed.
