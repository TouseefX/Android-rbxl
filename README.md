# rbxl Editor (Android, Rust)

A starter scaffold for editing scripts inside a `.rbxl` place file on
Android: parses/writes the place file with `rbx_binary`/`rbx_dom_weak`,
browses it with an Explorer-style tree in `egui`, and edits script `Source`
either inline (syntax-highlighted via `egui_code_editor`) or by handing the
script off to an external text editor app (QuickEdit etc.) and reading the
result back when you return to the app.
