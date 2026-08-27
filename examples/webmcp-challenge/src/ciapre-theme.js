import { tags } from "@lezer/highlight";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

export const CIAPRE_COLORS = Object.freeze({
  background: "#181818",
  foreground: "#aea47f",
  primary: "#aea47f",
  secondary: "#cc8a3e",
  alert: "#c16a68",
  surface: "#202020",
  surfaceRaised: "#262626",
  border: "#48402f",
  muted: "#8f896b",
  selection: "#4a3b25",
  added: "#b9c98a",
  removed: "#c16a68",
  alertForeground: "#d98582",
  keyword: "#e0b86a",
  string: "#b9c98a",
  number: "#d7b56d",
  comment: "#aaa27f",
  function: "#d8a262",
  type: "#c5a6d8",
  variable: "#d0c6a0",
});

export const ciapreHighlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: CIAPRE_COLORS.comment, fontStyle: "italic" },
  { tag: [tags.keyword, tags.operatorKeyword, tags.controlKeyword], color: CIAPRE_COLORS.keyword },
  { tag: [tags.string, tags.special(tags.string)], color: CIAPRE_COLORS.string },
  { tag: [tags.number, tags.bool, tags.null], color: CIAPRE_COLORS.number },
  { tag: [tags.function(tags.variableName), tags.labelName], color: CIAPRE_COLORS.function },
  { tag: [tags.typeName, tags.className, tags.namespace], color: CIAPRE_COLORS.type },
  { tag: [tags.variableName, tags.propertyName], color: CIAPRE_COLORS.variable },
  { tag: tags.definition(tags.variableName), color: CIAPRE_COLORS.primary },
  { tag: tags.meta, color: CIAPRE_COLORS.secondary },
]);

export const ciapreTheme = EditorView.theme({
  "&": { color: CIAPRE_COLORS.foreground, backgroundColor: CIAPRE_COLORS.background, height: "100%" },
  ".cm-scroller": { overflow: "auto", fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", fontSize: "13px" },
  ".cm-content": { caretColor: CIAPRE_COLORS.secondary, padding: "16px 0 24px" },
  ".cm-line": { padding: "0 18px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: CIAPRE_COLORS.secondary, borderLeftWidth: "2px" },
  ".cm-gutters": { backgroundColor: CIAPRE_COLORS.surface, color: CIAPRE_COLORS.muted, border: "0", borderRight: `1px solid ${CIAPRE_COLORS.border}` },
  ".cm-gutterElement": { padding: "0 10px 0 8px" },
  ".cm-activeLine": { backgroundColor: "#2a261f" },
  ".cm-activeLineGutter": { backgroundColor: "#2a261f", color: CIAPRE_COLORS.foreground },
  ".cm-selectionBackground, ::selection": { backgroundColor: `${CIAPRE_COLORS.selection} !important` },
  ".cm-focused .cm-selectionBackground": { backgroundColor: `${CIAPRE_COLORS.selection} !important` },
  ".cm-tooltip": { backgroundColor: CIAPRE_COLORS.surfaceRaised, color: CIAPRE_COLORS.foreground, border: `1px solid ${CIAPRE_COLORS.border}` },
  ".cm-panels": { backgroundColor: CIAPRE_COLORS.surfaceRaised, color: CIAPRE_COLORS.foreground },
});

export const editorDefaults = [
  EditorState.tabSize.of(4),
  EditorView.lineWrapping,
  ciapreTheme,
  syntaxHighlighting(ciapreHighlightStyle, { fallback: true }),
];
