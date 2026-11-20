# TextEditor

![Meeting notes in a multiline editor with a model preview below](screenshots/text_editor.png)

Run `plenty run --bin text_editor` in `examples/gui/iced_widgets`. Click the notes and press Enter to add a line. The preview below shows what the Plenty model accepted. With `run_events`, an edit proposes the whole document as `TextChanged(id, text)`. Cursor movement and selection do not change the model.

Keep a unique nonzero ID for each document. The binding keeps iced’s editing state for that ID while it remains present and its text agrees with the model. Replacing the document from Plenty or removing the editor resets that state. The visible editor is 120 logical pixels high. Documents and edit proposals are limited to 4,096 UTF-8 bytes; an oversized edit fails rather than silently truncating. This binding does not expose editing commands, syntax highlighting, or file I/O.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/text_editor.plenty}}
```
