import { EditorState } from "@codemirror/state";
import { EditorView, drawSelection, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { editorDefaults } from "./ciapre-theme.js";

const languageFor = (path) => {
  const ext = path.split(".").pop()?.toLowerCase();
  if (["js", "jsx", "mjs", "cjs", "ts", "tsx"].includes(ext)) return javascript({ typescript: ["ts", "tsx"].includes(ext) });
  return ({ md: markdown(), json: json(), css: css(), html: html(), htm: html(), rs: rust(), py: python(), pyw: python() })[ext] || [];
};

export class CodeEditor {
  constructor(parent, { onChange, onSave, onSelectionChange = () => {} }) {
    this.parent = parent;
    this.onChange = onChange;
    this.onSave = onSave;
    this.onSelectionChange = onSelectionChange;
    this.view = null;
    this.path = null;
  }

  open(path, content, dirty = false) {
    this.path = path;
    this.view?.destroy();
    const save = () => { this.onSave(path); return true; };
    const state = EditorState.create({
      doc: content,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        drawSelection(),
        keymap.of([{ key: "Mod-s", run: save }, ...defaultKeymap, indentWithTab]),
        languageFor(path),
        ...editorDefaults,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) this.onChange(path, update.state.doc.toString());
          if (update.selectionSet) this.onSelectionChange(path, update.state.selection);
        }),
      ],
    });
    this.view = new EditorView({ state, parent: this.parent });
    this.parent.classList.toggle("dirty-editor", dirty);
    this.view.focus();
  }

  get content() { return this.view?.state.doc.toString() || ""; }

  updateDirty(dirty) { this.parent.classList.toggle("dirty-editor", dirty); }

  focus() { this.view?.focus(); }

  destroy() { this.view?.destroy(); }
}
