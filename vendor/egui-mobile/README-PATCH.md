# Android TextEdit touch patch

This is egui 0.33.0, vendored under its upstream MIT/Apache-2.0 license.

OpenRBLX uses the patch through `[patch.crates-io]` because upstream `TextEdit`
gives a focused field drag ownership on touch screens. A full-screen focused
code editor therefore steals all swipes from its enclosing `ScrollArea` while
the Android IME is visible.

Local changes:

- `widgets/text_edit/builder.rs`: touch drags remain owned by the parent
  `ScrollArea`, regardless of editor focus.
- `text_selection/text_cursor_state.rs`: a long touch selects the touched word,
  preserving mobile selection and the app's Copy/Cut/Paste toolbar.

Desktop mouse drag-selection is unchanged.

Upstream source: https://github.com/emilk/egui tag `0.33.0`, commit
`28cbd73c5c0862c8a66321ebdfaebfce4839fabb`.
