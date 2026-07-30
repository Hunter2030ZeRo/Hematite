import {
  Compartment,
  EditorState,
  StateField,
  type Extension,
  type Text,
} from "@codemirror/state";
import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { lintGutter, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { Decoration, EditorView, WidgetType, hoverTooltip, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { tags } from "@lezer/highlight";
import { basicSetup } from "codemirror";
import { createEffect, onCleanup, onMount } from "solid-js";

type EditorSemanticToken = {
  kind: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type EditorHoverItem = {
  kind: string;
  title: string;
  detail?: string | null;
  source?: string | null;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type EditorHoverRequest = {
  path: string;
  content: string;
  line: number;
  column: number;
};

type EditorCompletionItem = {
  label: string;
  detail?: string | null;
  kind: string;
  insertText?: string | null;
};

type EditorCodeAction = {
  title: string;
  kind?: string | null;
  isPreferred: boolean;
};

type EditorLocation = {
  uri: string;
  path?: string | null;
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
};

type EditorInlayHint = {
  label: string;
  line: number;
  column: number;
  kind: string;
};

type CodeEditorProps = {
  value: string;
  path: string;
  diagnostics: Diagnostic[];
  semanticTokens: EditorSemanticToken[];
  inlayHints: EditorInlayHint[];
  lineWrapping: boolean;
  onHover?: (request: EditorHoverRequest) => Promise<EditorHoverItem | null>;
  onCompletions?: (request: EditorHoverRequest) => Promise<EditorCompletionItem[]>;
  onCodeActions?: (request: EditorHoverRequest) => Promise<EditorCodeAction[]>;
  onDefinition?: (request: EditorHoverRequest) => Promise<EditorLocation | null>;
  onReferences?: (request: EditorHoverRequest) => Promise<EditorLocation[]>;
  jumpToLine: number | null;
  onChange: (value: string) => void;
  onCursorChange?: (line: number, column: number) => void;
  onSave: () => void;
};

const languageCompartment = new Compartment();
const semanticCompartment = new Compartment();
const inlayHintCompartment = new Compartment();
const languageFeatureCompartment = new Compartment();
const hoverCompartment = new Compartment();
const lineWrappingCompartment = new Compartment();

const hematiteSyntaxHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    {
      tag: [
        tags.keyword,
        tags.controlKeyword,
        tags.definitionKeyword,
        tags.moduleKeyword,
        tags.modifier,
        tags.operatorKeyword,
        tags.self,
        tags.null,
        tags.atom,
      ],
      color: "#c7b8d8",
      fontWeight: "600",
    },
    {
      tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment],
      color: "#777d86",
      fontStyle: "italic",
    },
    {
      tag: [tags.string, tags.docString, tags.character, tags.attributeValue],
      color: "#afc8b3",
    },
    {
      tag: [tags.number, tags.integer, tags.float, tags.bool],
      color: "#d2c49e",
    },
    {
      tag: [tags.escape, tags.regexp, tags.special(tags.string)],
      color: "#d3abb7",
    },
    {
      tag: [
        tags.function(tags.variableName),
        tags.definition(tags.function(tags.variableName)),
        tags.function(tags.propertyName),
      ],
      color: "#b7d1dd",
      fontWeight: "600",
    },
    {
      tag: [tags.className, tags.typeName, tags.tagName, tags.definition(tags.typeName)],
      color: "#b4c5dc",
      fontWeight: "600",
    },
    {
      tag: [tags.namespace, tags.macroName, tags.labelName],
      color: "#a8cabe",
    },
    {
      tag: [tags.propertyName, tags.attributeName],
      color: "#c0cfd6",
    },
    {
      tag: [
        tags.variableName,
        tags.definition(tags.variableName),
        tags.local(tags.variableName),
        tags.standard(tags.variableName),
        tags.special(tags.variableName),
      ],
      color: "#e6e8eb",
    },
    {
      tag: [
        tags.operator,
        tags.arithmeticOperator,
        tags.logicOperator,
        tags.compareOperator,
        tags.definitionOperator,
        tags.typeOperator,
        tags.controlOperator,
        tags.derefOperator,
      ],
      color: "#c2c5ca",
    },
    {
      tag: [tags.punctuation, tags.separator, tags.bracket, tags.paren, tags.squareBracket, tags.brace],
      color: "#969ba3",
    },
    {
      tag: [tags.meta, tags.processingInstruction],
      color: "#d4b5a3",
    },
    {
      tag: tags.invalid,
      color: "#d69aa4",
      textDecoration: "underline wavy rgba(214, 154, 164, 0.72)",
    },
  ])
);

const editorTheme = EditorView.theme(
  {
    "&": {
      height: "100%",
      "background-color": "#0e1013",
      color: "#e7e9ec",
      "font-family": '"Cascadia Code", "Cascadia Mono", "SFMono-Regular", Consolas, monospace',
      "font-size": "15.5px",
      "font-weight": "400",
      "-webkit-font-smoothing": "antialiased",
    },
    ".cm-scroller": {
      "line-height": "1.5",
      overflow: "auto",
      "overscroll-behavior": "contain",
    },
    ".cm-content": {
      padding: "16px 0 48px",
      "caret-color": "#f0f1f3",
    },
    ".cm-line": {
      padding: "0 20px",
    },
    ".cm-gutters": {
      "background-color": "#0a0c0f",
      color: "#686d75",
      border: "none",
      "padding-right": "10px",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      "padding-left": "8px",
      "padding-right": "12px",
    },
    ".cm-activeLine": {
      "background-color": "rgba(255, 255, 255, 0.035)",
    },
    ".cm-activeLineGutter": {
      "background-color": "#0a0c0f",
      color: "#cfd2d6",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      "background-color": "rgba(174, 188, 210, 0.22)",
    },
    ".cm-selectionMatch": {
      "background-color": "rgba(174, 188, 210, 0.11)",
    },
    ".cm-cursor, .cm-dropCursor": {
      "border-left-color": "#eceef1",
      "border-left-width": "2px",
    },
    ".cm-matchingBracket": {
      "background-color": "rgba(183, 209, 221, 0.1)",
      color: "#dce6ea",
      outline: "1px solid rgba(183, 209, 221, 0.35)",
    },
    ".cm-nonmatchingBracket": {
      "background-color": "rgba(214, 154, 164, 0.1)",
      color: "#d9a4aa",
      outline: "1px solid rgba(214, 154, 164, 0.38)",
    },
    ".cm-tooltip": {
      "background-color": "#171a1f",
      border: "1px solid rgba(255,255,255,0.11)",
      color: "#e2e5e9",
      "box-shadow": "0 10px 30px rgba(0, 0, 0, 0.34)",
      "max-width": "min(720px, 72vw)",
      "max-height": "min(440px, 56vh)",
      overflow: "hidden",
    },
    ".cm-panels": {
      "background-color": "#0d0f12",
      color: "#d9dce0",
      border: "none",
    },
    ".cm-tooltip.cm-tooltip-autocomplete": {
      "background-color": "#15181d",
      border: "1px solid rgba(255,255,255,0.1)",
      color: "#e2e5e9",
    },
    ".cm-tooltip-autocomplete > ul": {
      "font-family": '"Cascadia Code", "Cascadia Mono", "SFMono-Regular", Consolas, monospace',
    },
    ".cm-tooltip-autocomplete > ul > li": {
      color: "#d5d8dd",
      padding: "6px 10px",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      "background-color": "rgba(255, 255, 255, 0.09)",
      color: "#f3f4f5",
    },
    ".cm-completionMatchedText": {
      color: "#b7d1dd",
      "text-decoration": "none",
      "font-weight": "700",
    },
    ".cm-diagnosticText": {
      "font-family": '"IBM Plex Sans", "Segoe UI", sans-serif',
    },
    ".cm-inlay-hint": {
      "margin-left": "6px",
      padding: "0 5px",
      "border-radius": "4px",
      "background-color": "rgba(255, 255, 255, 0.055)",
      color: "#969ca5",
      "font-size": "12px",
      "font-style": "normal",
      "vertical-align": "baseline",
    },
    ".cm-content .cm-semantic-keyword, .cm-content .cm-semantic-keyword *, .cm-content .cm-semantic-modifier, .cm-content .cm-semantic-modifier *": {
      color: "#c7b8d8 !important",
    },
    ".cm-content .cm-semantic-string, .cm-content .cm-semantic-string *": {
      color: "#afc8b3 !important",
    },
    ".cm-content .cm-semantic-number, .cm-content .cm-semantic-number *": {
      color: "#d2c49e !important",
    },
    ".cm-content .cm-semantic-operator, .cm-content .cm-semantic-operator *": {
      color: "#c2c5ca !important",
    },
    ".cm-content .cm-semantic-comment, .cm-content .cm-semantic-comment *": {
      color: "#777d86 !important",
      "font-style": "italic",
    },
    ".cm-content .cm-semantic-namespace, .cm-content .cm-semantic-module, .cm-content .cm-semantic-namespace *, .cm-content .cm-semantic-module *": {
      color: "#a8cabe !important",
    },
    ".cm-content .cm-semantic-function, .cm-content .cm-semantic-function *, .cm-content .cm-semantic-functionDefinition, .cm-content .cm-semantic-functionDefinition *, .cm-content .cm-semantic-methodDefinition, .cm-content .cm-semantic-methodDefinition *":
      {
        color: "#b7d1dd !important",
      },
    ".cm-content .cm-semantic-method, .cm-content .cm-semantic-method *, .cm-content .cm-semantic-functionCall, .cm-content .cm-semantic-functionCall *, .cm-content .cm-semantic-methodCall, .cm-content .cm-semantic-methodCall *":
      {
        color: "#d2c49e !important",
      },
    ".cm-content .cm-semantic-macro, .cm-content .cm-semantic-macro *, .cm-content .cm-semantic-attribute, .cm-content .cm-semantic-attribute *":
      {
        color: "#c7b8d8 !important",
      },
    ".cm-content .cm-semantic-class, .cm-content .cm-semantic-class *, .cm-content .cm-semantic-type, .cm-content .cm-semantic-type *, .cm-content .cm-semantic-classDefinition, .cm-content .cm-semantic-classDefinition *, .cm-content .cm-semantic-classReference, .cm-content .cm-semantic-classReference *":
      {
        color: "#b4c5dc !important",
      },
    ".cm-content .cm-semantic-struct, .cm-content .cm-semantic-struct *, .cm-content .cm-semantic-enum, .cm-content .cm-semantic-enum *, .cm-content .cm-semantic-enumMember, .cm-content .cm-semantic-enumMember *, .cm-content .cm-semantic-builtinType, .cm-content .cm-semantic-builtinType *":
      {
        color: "#b4c5dc !important",
      },
    ".cm-content .cm-semantic-lifetime, .cm-content .cm-semantic-lifetime *": {
      color: "#d4b5a3 !important",
    },
    ".cm-content .cm-semantic-unresolvedReference, .cm-content .cm-semantic-unresolvedReference *":
      {
        color: "#d69aa4 !important",
        "text-decoration": "underline wavy rgba(214, 154, 164, 0.72)",
      },
    ".cm-content .cm-semantic-parameter, .cm-content .cm-semantic-parameter *": {
      color: "#d4b5a3 !important",
    },
    ".cm-content .cm-semantic-variableDefinition, .cm-content .cm-semantic-variableDefinition *":
      {
        color: "#d7c3ab !important",
      },
    ".cm-content .cm-semantic-variable, .cm-content .cm-semantic-variable *, .cm-content .cm-semantic-identifier, .cm-content .cm-semantic-identifier *": {
      color: "#e6e8eb !important",
    },
    ".cm-content .cm-semantic-property, .cm-content .cm-semantic-property *": {
      color: "#c0cfd6 !important",
    },
    ".hematite-hover": {
      display: "grid",
      gap: "8px",
      "max-width": "min(700px, 70vw)",
      "max-height": "min(420px, 54vh)",
      padding: "4px",
      overflow: "auto",
    },
    ".hematite-hover-head": {
      display: "flex",
      "align-items": "center",
      gap: "8px",
      "min-width": 0,
    },
    ".hematite-hover-title": {
      color: "#e8eaed",
      "font-family": '"Cascadia Code", "Cascadia Mono", "SFMono-Regular", Consolas, monospace',
      "font-size": "12px",
      "font-weight": "600",
      "line-height": "1.45",
      "white-space": "pre-wrap",
      "overflow-wrap": "anywhere",
    },
    ".hematite-hover-kind": {
      color: "#b7c8dc",
      "font-size": "11px",
      "font-weight": "700",
      "letter-spacing": "0.04em",
      "text-transform": "uppercase",
    },
    ".hematite-hover-detail": {
      color: "#c8ccd2",
      "font-family": '"IBM Plex Sans", "Segoe UI", sans-serif',
      "font-size": "12px",
      "line-height": "1.5",
      "white-space": "pre-wrap",
      "overflow-wrap": "anywhere",
    },
    ".hematite-hover-source": {
      color: "#858b94",
      "font-family": '"IBM Plex Sans", "Segoe UI", sans-serif',
      "font-size": "11px",
      "line-height": "1.4",
      "overflow-wrap": "anywhere",
    },
  },
  { dark: true }
);

function positionFromLineColumn(doc: Text, lineNumber: number, columnNumber: number) {
  const safeLineNumber = Math.min(Math.max(1, lineNumber), Math.max(1, doc.lines));
  const line = doc.line(safeLineNumber);
  const safeColumn = Math.max(1, columnNumber);
  return Math.min(line.from + safeColumn - 1, line.to);
}

function semanticDecorations(tokens: EditorSemanticToken[]): Extension {
  if (!tokens.length) {
    return [];
  }

  return StateField.define({
    create(state) {
      const ranges = tokens
        .map((token) => {
          const from = positionFromLineColumn(state.doc, token.startLine, token.startColumn);
          const to = Math.max(
            from + 1,
            positionFromLineColumn(state.doc, token.endLine, token.endColumn)
          );

          return Decoration.mark({
            class: `cm-semantic-${token.kind}`,
          }).range(from, to);
        })
        .sort((left, right) => left.from - right.from || left.to - right.to);

      return Decoration.set(ranges, true);
    },
    update(value, transaction) {
      if (!transaction.docChanged) {
        return value;
      }

      return value.map(transaction.changes);
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

class InlayHintWidget extends WidgetType {
  constructor(
    private readonly label: string,
    private readonly kind: string
  ) {
    super();
  }

  toDOM() {
    const span = document.createElement("span");
    span.className = `cm-inlay-hint cm-inlay-hint-${this.kind}`;
    span.textContent = this.label;
    return span;
  }

  ignoreEvent() {
    return true;
  }
}

function inlayHintDecorations(hints: EditorInlayHint[]): Extension {
  if (!hints.length) {
    return [];
  }

  return StateField.define({
    create(state) {
      const ranges = hints
        .map((hint) => {
          const pos = positionFromLineColumn(state.doc, hint.line, hint.column);
          return Decoration.widget({
            widget: new InlayHintWidget(hint.label, hint.kind),
            side: 1,
          }).range(pos);
        })
        .sort((left, right) => left.from - right.from || left.to - right.to);

      return Decoration.set(ranges, true);
    },
    update(value, transaction) {
      if (!transaction.docChanged) {
        return value;
      }

      return value.map(transaction.changes);
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

function editorFeatureRequest(view: EditorView, path: string, pos = view.state.selection.main.head) {
  const line = view.state.doc.lineAt(pos);
  return {
    path,
    content: view.state.doc.toString(),
    line: line.number,
    column: pos - line.from + 1,
  };
}

function completionType(kind: string) {
  switch (kind) {
    case "class":
    case "function":
    case "method":
    case "variable":
    case "keyword":
    case "module":
    case "property":
      return kind;
    default:
      return "variable";
  }
}

function pythonCompletions(
  path: string,
  onCompletions: CodeEditorProps["onCompletions"]
) {
  return async (context: CompletionContext) => {
    if (!onCompletions || !path.toLowerCase().endsWith(".py")) {
      return null;
    }

    const token = context.matchBefore(/[A-Za-z_][\w.]*/);
    if (!context.explicit && (!token || token.from === token.to)) {
      return null;
    }

    const items = await onCompletions(editorFeatureRequest(context.view, path, context.pos));
    if (!items.length) {
      return null;
    }

    return {
      from: token?.from ?? context.pos,
      options: items.map((item) => ({
        label: item.label,
        type: completionType(item.kind),
        detail: item.detail ?? undefined,
        apply: item.insertText ?? item.label,
      })),
    };
  };
}

function languageFeatureExtensions(props: CodeEditorProps): Extension {
  const commands = [
    {
      key: "F12",
      run: (view: EditorView) => {
        if (!props.onDefinition) {
          return false;
        }
        void props.onDefinition(editorFeatureRequest(view, props.path));
        return true;
      },
    },
    {
      key: "Shift-F12",
      run: (view: EditorView) => {
        if (!props.onReferences) {
          return false;
        }
        void props.onReferences(editorFeatureRequest(view, props.path));
        return true;
      },
    },
    {
      key: "Mod-.",
      run: (view: EditorView) => {
        if (!props.onCodeActions) {
          return false;
        }
        void props.onCodeActions(editorFeatureRequest(view, props.path));
        return true;
      },
    },
  ];

  return [
    autocompletion({
      override: [pythonCompletions(props.path, props.onCompletions)],
    }),
    keymap.of(commands),
  ];
}

function hoverTooltips(
  path: string,
  content: string,
  onHover: CodeEditorProps["onHover"]
): Extension {
  if (!onHover) {
    return [];
  }

  return hoverTooltip(
    async (view, pos) => {
      const line = view.state.doc.lineAt(pos);
      const match = await onHover({
        path,
        content,
        line: line.number,
        column: pos - line.from + 1,
      });

      if (!match?.title && !match?.detail) {
        return null;
      }

      const word = view.state.wordAt(pos);
      const from = word?.from ?? pos;
      const to = word?.to ?? pos + 1;

      return {
        pos: from,
        end: to,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "hematite-hover";

          const head = document.createElement("div");
          head.className = "hematite-hover-head";

          const kind = document.createElement("span");
          kind.className = "hematite-hover-kind";
          kind.textContent = match.kind || "Language";
          head.append(kind);

          const title = document.createElement("div");
          title.className = "hematite-hover-title";
          title.textContent = match.title || "Symbol";
          head.append(title);
          dom.append(head);

          if (match.detail) {
            const detail = document.createElement("div");
            detail.className = "hematite-hover-detail";
            detail.textContent = match.detail;
            dom.append(detail);
          }

          if (match.source) {
            const source = document.createElement("div");
            source.className = "hematite-hover-source";
            source.textContent = match.source;
            dom.append(source);
          }

          return { dom };
        },
      };
    },
    { hoverTime: 320, hideOnChange: true }
  );
}

function emitCursorPosition(view: EditorView, onCursorChange?: CodeEditorProps["onCursorChange"]) {
  if (!onCursorChange) {
    return;
  }

  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  onCursorChange(line.number, head - line.from + 1);
}

async function languageExtensionForPath(path: string): Promise<Extension> {
  const extension = path.split(".").pop()?.toLowerCase();

  switch (extension) {
    case "py": {
      const { python } = await import("@codemirror/lang-python");
      return python();
    }
    case "rs": {
      const { rust } = await import("@codemirror/lang-rust");
      return rust();
    }
    case "c":
    case "h":
    case "cc":
    case "cpp":
    case "cxx":
    case "hh":
    case "hpp":
    case "hxx":
    case "cu":
    case "cuh": {
      const { cpp } = await import("@codemirror/lang-cpp");
      return cpp();
    }
    case "ts": {
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript({ typescript: true });
    }
    case "tsx": {
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript({ typescript: true, jsx: true });
    }
    case "js":
    case "mjs":
    case "cjs": {
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript();
    }
    case "jsx": {
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript({ jsx: true });
    }
    case "json": {
      const { json } = await import("@codemirror/lang-json");
      return json();
    }
    case "css": {
      const { css } = await import("@codemirror/lang-css");
      return css();
    }
    case "html":
    case "htm": {
      const { html } = await import("@codemirror/lang-html");
      return html();
    }
    case "md": {
      const { markdown } = await import("@codemirror/lang-markdown");
      return markdown();
    }
    default:
      return [];
  }
}

export default function CodeEditor(props: CodeEditorProps) {
  let host!: HTMLDivElement;
  let view: EditorView | undefined;
  let languageLoadVersion = 0;

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: props.value,
        extensions: [
          basicSetup,
          hematiteSyntaxHighlighting,
          editorTheme,
          lintGutter(),
          keymap.of([
            indentWithTab,
            {
              key: "Mod-s",
              preventDefault: true,
              run: () => {
                props.onSave();
                return true;
              },
            },
          ]),
          languageCompartment.of([]),
          semanticCompartment.of([]),
          inlayHintCompartment.of([]),
          languageFeatureCompartment.of(languageFeatureExtensions(props)),
          hoverCompartment.of([]),
          lineWrappingCompartment.of(props.lineWrapping ? EditorView.lineWrapping : []),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              props.onChange(update.state.doc.toString());
            }
            if (update.docChanged || update.selectionSet) {
              emitCursorPosition(update.view, props.onCursorChange);
            }
          }),
        ],
      }),
      parent: host,
    });

    view.dispatch(setDiagnostics(view.state, props.diagnostics));
    emitCursorPosition(view, props.onCursorChange);
  });

  createEffect(() => {
    const nextValue = props.value;
    if (!view || nextValue === view.state.doc.toString()) {
      return;
    }

    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: nextValue,
      },
    });
  });

  createEffect(() => {
    const path = props.path;
    const currentVersion = ++languageLoadVersion;

    void (async () => {
      const extension = await languageExtensionForPath(path);
      if (!view || currentVersion !== languageLoadVersion) {
        return;
      }

      view.dispatch({
        effects: languageCompartment.reconfigure(extension),
      });
    })();
  });

  createEffect(() => {
    if (!view) {
      return;
    }

    view.dispatch(setDiagnostics(view.state, props.diagnostics));
  });

  createEffect(() => {
    props.semanticTokens;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: semanticCompartment.reconfigure(semanticDecorations(props.semanticTokens)),
    });
  });

  createEffect(() => {
    props.inlayHints;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: inlayHintCompartment.reconfigure(inlayHintDecorations(props.inlayHints)),
    });
  });

  createEffect(() => {
    props.path;
    props.onCompletions;
    props.onCodeActions;
    props.onDefinition;
    props.onReferences;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: languageFeatureCompartment.reconfigure(languageFeatureExtensions(props)),
    });
  });

  createEffect(() => {
    const path = props.path;
    const content = props.value;
    const onHover = props.onHover;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: hoverCompartment.reconfigure(hoverTooltips(path, content, onHover)),
    });
  });

  createEffect(() => {
    const enabled = props.lineWrapping;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: lineWrappingCompartment.reconfigure(enabled ? EditorView.lineWrapping : []),
    });
  });

  createEffect(() => {
    if (!view || props.jumpToLine == null) {
      return;
    }

    const lineNumber = Math.min(
      Math.max(1, props.jumpToLine),
      Math.max(1, view.state.doc.lines)
    );
    const line = view.state.doc.line(lineNumber);
    view.dispatch({
      selection: { anchor: line.from },
      effects: EditorView.scrollIntoView(line.from, { y: "center" }),
    });
    view.focus();
  });

  onCleanup(() => {
    view?.destroy();
    view = undefined;
  });

  return <div class="editor-host" ref={host} />;
}
