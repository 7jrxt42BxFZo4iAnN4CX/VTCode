import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { EditorState, type EditorSelection, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  type ViewUpdate,
} from "@codemirror/view";
import { editorDefaults } from "./ciapre-theme.ts";

const languageFactories: Record<string, () => Extension> = {
  md: () => markdown(),
  json: () => json(),
  css: () => css(),
  html: () => html(),
  htm: () => html(),
  rs: () => rust(),
  py: () => python(),
  pyw: () => python(),
};

function languageFor(path: string): Extension {
  const extension = path.split(".").pop()?.toLowerCase();
  if (extension && ["js", "jsx", "mjs", "cjs", "ts", "tsx"].includes(extension)) {
    return javascript({ typescript: ["ts", "tsx"].includes(extension) });
  }
  return extension && languageFactories[extension] ? languageFactories[extension]() : [];
}

interface CodeEditorOptions {
  readonly onChange: (path: string, content: string) => void;
  readonly onSave: (path: string) => void;
  readonly onSelectionChange?: (path: string, selection: EditorSelection) => void;
}

export class CodeEditor {
  readonly parent: HTMLElement;
  readonly onChange: (path: string, content: string) => void;
  readonly onSave: (path: string) => void;
  readonly onSelectionChange: (path: string, selection: EditorSelection) => void;
  view: EditorView | null = null;
  path: string | null = null;

  constructor(parent: HTMLElement, { onChange, onSave, onSelectionChange = () => {} }: CodeEditorOptions) {
    this.parent = parent;
    this.onChange = onChange;
    this.onSave = onSave;
    this.onSelectionChange = onSelectionChange;
  }

  open(path: string, content: string, dirty = false): void {
    this.path = path;
    this.view?.destroy();
    const save = (): boolean => {
      this.onSave(path);
      return true;
    };
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
        EditorView.updateListener.of((update: ViewUpdate) => {
          if (update.docChanged) this.onChange(path, update.state.doc.toString());
          if (update.selectionSet) this.onSelectionChange(path, update.state.selection);
        }),
      ],
    });
    this.view = new EditorView({ state, parent: this.parent });
    this.parent.classList.toggle("dirty-editor", dirty);
    this.view.focus();
  }

  get content(): string { return this.view?.state.doc.toString() || ""; }

  updateDirty(dirty: boolean): void { this.parent.classList.toggle("dirty-editor", dirty); }

  focus(): void { this.view?.focus(); }

  destroy(): void { this.view?.destroy(); }
}
