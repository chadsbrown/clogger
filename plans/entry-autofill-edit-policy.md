# Entry Autofill Edit Policy

## Goal

Make editing behavior for pre-populated entry fields explicit and configurable.

In contest operation, when an exchange field is pre-filled from call history or
contest history, the first printable character typed by the operator should
usually replace the entire pre-filled value. The operator should not need to
backspace over stale history data during a QSO.

This behavior should be configurable.

## Current Behavior

Entry fields have a `from_history` flag.

For non-call fields, the reducer currently does this on `TextInput`:

- If the focused field has `from_history = true`, clear the field.
- Set cursor to 0.
- Insert the typed text.
- Clear `from_history`.

So history-filled exchange fields already behave like "replace on first typed
character."

The CALL field is treated differently. Typing in CALL is what triggers history
lookup, so the replacement behavior is skipped for CALL.

Tab and Space only move focus. They do not clear values by themselves.

Backspace does not clear the whole history-filled value. It clears the
`from_history` flag and deletes one character before the cursor.

The GUI entry pane is not currently a set of clickable text input widgets. It
renders fields and a caret, while keyboard input goes through the reducer. So
mouse clicking a field should not be the primary cause of replace-vs-append
behavior.

## Likely Source Of Inconsistent Behavior

If a visible pre-filled value is not tagged with `from_history = true`, first
typing will append or insert normally instead of replacing the whole value.

Append-like behavior may happen when:

- The field was pre-populated by something other than call/contest history.
- The field appears pre-filled but lacks the `from_history` marker.
- The operator already typed or backspaced once, which clears `from_history`.
- A value is considered manual and should not be overwritten by later history.
- The cursor is at the end because history population sets `cursor =
  value.len()`.

The current `from_history` flag is doing two jobs:

- It marks text as auto-filled, so first printable typing should replace it.
- It marks text as eligible to be overwritten by future history refreshes.

Those concepts are related but not identical.

## Desired Model

Separate field value provenance from edit behavior.

Possible future shape:

```rust
enum FieldOrigin {
    Empty,
    Manual,
    CallHistory,
    ContestHistory,
    Default,
    Bandmap,
    Generated,
}

enum AutofillEditPolicy {
    ReplaceOnFirstType,
    AppendOnType,
    Normal,
}
```

The reducer can then reason clearly:

- Was this value auto-filled?
- Should first printable typing replace it?
- Is this value still eligible for future history overwrite?

This avoids overloading `from_history` with multiple meanings.

## Config Direction

Start simple:

```toml
[entry]
autofill_edit_behavior = "replace_on_first_type"
```

Possible values:

- `replace_on_first_type`
- `append`

If needed later, make it more granular:

```toml
[entry.autofill]
call_history = "replace_on_first_type"
contest_history = "replace_on_first_type"
defaults = "normal"
bandmap = "normal"
```

The likely default should be:

- History-filled exchange fields: `replace_on_first_type`
- Manually edited fields: `normal`
- CALL field: `normal`

## Backspace Behavior

Backspace should probably remain an explicit edit operation:

- It should delete one character before the cursor.
- It should mark the field as manually edited.
- It should not necessarily clear the whole pre-filled field.

Printable typing is the operation that should optionally replace the full
auto-filled value.

## Testing Notes

Add focused reducer tests for:

- Call history pre-fills an exchange field; first printable typing replaces it.
- Contest history pre-fills an exchange field; first printable typing replaces it.
- Backspace in a history-filled field deletes one character, not the whole field.
- Manual edit survives subsequent history refresh after CALL changes.
- Non-history defaults do not accidentally get history replacement behavior
  unless configured.
- Any future prefill sources are consistently tagged with their origin.

## Constraints

- Do not make the operator backspace through stale history during a QSO.
- Do not let history refresh clobber a manual correction.
- Do not infer behavior from mouse versus Tab/Space focus movement.
- Keep the behavior reducer-driven so TUI and GUI stay consistent.

