import {
  Compartment,
  EditorState,
  StateField,
  type Extension,
  type Text,
} from "@codemirror/state";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { lintGutter, setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { Decoration, EditorView, hoverTooltip, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { basicSetup } from "codemirror";
import { tags } from "@lezer/highlight";
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

type CodeEditorProps = {
  value: string;
  path: string;
  diagnostics: Diagnostic[];
  semanticTokens: EditorSemanticToken[];
  hoverItems: EditorHoverItem[];
  jumpToLine: number | null;
  onChange: (value: string) => void;
  onSave: () => void;
};

const languageCompartment = new Compartment();
const semanticCompartment = new Compartment();
const hoverCompartment = new Compartment();

const editorHighlightStyle = HighlightStyle.define([
  {
    tag: [
      tags.keyword,
      tags.modifier,
      tags.controlKeyword,
      tags.definitionKeyword,
      tags.moduleKeyword,
      tags.operatorKeyword,
      tags.controlOperator,
    ],
    color: "#ff9e64",
  },
  { tag: [tags.atom, tags.bool, tags.null], color: "#ffb870" },
  { tag: [tags.number, tags.integer, tags.float], color: "#ffd479" },
  { tag: [tags.string, tags.special(tags.string)], color: "#9fe870" },
  { tag: [tags.regexp, tags.escape], color: "#73e2d1" },
  {
    tag: [tags.comment, tags.lineComment, tags.blockComment],
    color: "#90a4c0",
    fontStyle: "italic",
  },
  { tag: [tags.variableName, tags.self], color: "#edf3ff" },
  { tag: [tags.definition(tags.variableName), tags.labelName], color: "#ffca8f" },
  { tag: [tags.namespace], color: "#57dcc4" },
  { tag: [tags.typeName, tags.className], color: "#8cb8ff" },
  { tag: [tags.propertyName, tags.attributeName], color: "#86dfff" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "#ffd166" },
  { tag: [tags.definition(tags.function(tags.variableName))], color: "#7fe3ff" },
  { tag: [tags.operator, tags.punctuation, tags.separator], color: "#d6deea" },
  { tag: [tags.meta, tags.annotation], color: "#ffc08f" },
  { tag: [tags.heading], color: "#8fddff", fontWeight: "700" },
  { tag: [tags.emphasis], fontStyle: "italic" },
  { tag: [tags.strong], fontWeight: "700" },
  { tag: [tags.link], color: "#8fddff", textDecoration: "underline" },
]);

const editorTheme = EditorView.theme(
  {
    "&": {
      height: "100%",
      "background-color": "#171b23",
      color: "#f4f8ff",
      "font-family": '"JetBrains Mono", "IBM Plex Mono", Consolas, monospace',
      "font-size": "14px",
      "font-weight": "500",
      "-webkit-font-smoothing": "antialiased",
    },
    ".cm-scroller": {
      "line-height": "1.72",
      overflow: "auto",
      "overscroll-behavior": "contain",
    },
    ".cm-content": {
      padding: "16px 0 48px",
      "caret-color": "#9ecbff",
    },
    ".cm-line": {
      padding: "0 20px",
    },
    ".cm-gutters": {
      "background-color": "#11151d",
      color: "#95a6be",
      border: "none",
      "padding-right": "10px",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      "padding-left": "8px",
      "padding-right": "12px",
    },
    ".cm-activeLine": {
      "background-color": "rgba(143, 221, 255, 0.1)",
    },
    ".cm-activeLineGutter": {
      "background-color": "#11151d",
      color: "#f6faff",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      "background-color": "rgba(92, 159, 255, 0.42)",
    },
    ".cm-selectionMatch": {
      "background-color": "rgba(92, 159, 255, 0.2)",
    },
    ".cm-cursor, .cm-dropCursor": {
      "border-left-color": "#b4d5ff",
      "border-left-width": "2px",
    },
    ".cm-matchingBracket": {
      "background-color": "rgba(166, 218, 149, 0.14)",
      color: "#d8f5bf",
      outline: "1px solid rgba(166, 218, 149, 0.45)",
    },
    ".cm-nonmatchingBracket": {
      "background-color": "rgba(255, 122, 182, 0.12)",
      color: "#ff9fc8",
      outline: "1px solid rgba(255, 122, 182, 0.4)",
    },
    ".cm-tooltip": {
      "background-color": "#222a36",
      border: "1px solid rgba(255,255,255,0.11)",
      color: "#eef4fb",
      "box-shadow": "0 10px 30px rgba(0, 0, 0, 0.34)",
    },
    ".cm-panels": {
      "background-color": "#10151d",
      color: "#e4edf8",
      border: "none",
    },
    ".cm-tooltip.cm-tooltip-autocomplete": {
      "background-color": "#1b2330",
      border: "1px solid rgba(255,255,255,0.1)",
      color: "#eef4fb",
    },
    ".cm-tooltip-autocomplete > ul": {
      "font-family": '"JetBrains Mono", "IBM Plex Mono", Consolas, monospace',
    },
    ".cm-tooltip-autocomplete > ul > li": {
      color: "#dfe8f5",
      padding: "6px 10px",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      "background-color": "rgba(87, 220, 196, 0.18)",
      color: "#f8fbff",
    },
    ".cm-completionMatchedText": {
      color: "#86e1fc",
      "text-decoration": "none",
      "font-weight": "700",
    },
    ".cm-diagnosticText": {
      "font-family": '"IBM Plex Sans", "Segoe UI", sans-serif',
    },
    ".cm-content .cm-semantic-namespace, .cm-content .cm-semantic-namespace *": {
      color: "#57dcc4 !important",
    },
    ".cm-content .cm-semantic-functionDefinition, .cm-content .cm-semantic-functionDefinition *, .cm-content .cm-semantic-methodDefinition, .cm-content .cm-semantic-methodDefinition *":
      {
        color: "#86e1fc !important",
      },
    ".cm-content .cm-semantic-functionCall, .cm-content .cm-semantic-functionCall *, .cm-content .cm-semantic-methodCall, .cm-content .cm-semantic-methodCall *":
      {
        color: "#ffd479 !important",
      },
    ".cm-content .cm-semantic-classDefinition, .cm-content .cm-semantic-classDefinition *, .cm-content .cm-semantic-classReference, .cm-content .cm-semantic-classReference *":
      {
        color: "#8cb8ff !important",
      },
    ".cm-content .cm-semantic-parameter, .cm-content .cm-semantic-parameter *": {
      color: "#ffbe7a !important",
    },
    ".cm-content .cm-semantic-variableDefinition, .cm-content .cm-semantic-variableDefinition *":
      {
        color: "#ffd29c !important",
      },
    ".cm-content .cm-semantic-variable, .cm-content .cm-semantic-variable *": {
      color: "#eef5ff !important",
    },
    ".cm-content .cm-semantic-property, .cm-content .cm-semantic-property *": {
      color: "#9adfff !important",
    },
    ".hematite-hover": {
      display: "grid",
      gap: "8px",
      "max-width": "420px",
      padding: "2px",
    },
    ".hematite-hover-head": {
      display: "flex",
      "align-items": "center",
      gap: "8px",
      "min-width": 0,
    },
    ".hematite-hover-title": {
      color: "#f3f8ff",
      "font-family": '"JetBrains Mono", "IBM Plex Mono", Consolas, monospace',
      "font-size": "12px",
      "font-weight": "600",
      "line-height": "1.5",
      "overflow-wrap": "anywhere",
    },
    ".hematite-hover-kind": {
      color: "#7cc9ff",
      "font-size": "11px",
      "font-weight": "700",
      "letter-spacing": "0.04em",
      "text-transform": "uppercase",
    },
    ".hematite-hover-detail": {
      color: "#c8d7ea",
      "font-family": '"IBM Plex Sans", "Segoe UI", sans-serif',
      "font-size": "12px",
      "line-height": "1.55",
      "white-space": "pre-wrap",
      "overflow-wrap": "anywhere",
    },
    ".hematite-hover-source": {
      color: "#8ea4c1",
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

function hoverSummary(item: EditorHoverItem) {
  return [item.kind, item.title, item.detail, item.source].filter(Boolean).join("\n");
}

function semanticDecorations(
  tokens: EditorSemanticToken[],
  hoverItems: EditorHoverItem[]
): Extension {
  if (!tokens.length && !hoverItems.length) {
    return [];
  }

  return StateField.define({
    create(state) {
      const ranges = [
        ...tokens.map((token) => {
          const from = positionFromLineColumn(state.doc, token.startLine, token.startColumn);
          const to = Math.max(
            from + 1,
            positionFromLineColumn(state.doc, token.endLine, token.endColumn)
          );
          const hover = hoverItems.find(
            (item) =>
              item.startLine === token.startLine &&
              item.startColumn === token.startColumn &&
              item.endLine === token.endLine &&
              item.endColumn === token.endColumn
          );

          return Decoration.mark({
            class: `cm-semantic-${token.kind}`,
            attributes: hover ? { title: hoverSummary(hover) } : {},
          }).range(from, to);
        }),
        ...hoverItems
          .filter(
            (item) =>
              !tokens.some(
                (token) =>
                  token.startLine === item.startLine &&
                  token.startColumn === item.startColumn &&
                  token.endLine === item.endLine &&
                  token.endColumn === item.endColumn
              )
          )
          .map((item) => {
            const from = positionFromLineColumn(state.doc, item.startLine, item.startColumn);
            const to = Math.max(
              from + 1,
              positionFromLineColumn(state.doc, item.endLine, item.endColumn)
            );
            return Decoration.mark({
              class: "cm-semantic-hoverTarget",
              attributes: { title: hoverSummary(item) },
            }).range(from, to);
          }),
      ].sort((left, right) => left.from - right.from || left.to - right.to);

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

function hoverTooltips(items: EditorHoverItem[]): Extension {
  if (!items.length) {
    return [];
  }

  return hoverTooltip(
    (view, pos) => {
      const match = items.find((item) => {
        const from = positionFromLineColumn(view.state.doc, item.startLine, item.startColumn);
        const to = Math.max(
          from + 1,
          positionFromLineColumn(view.state.doc, item.endLine, item.endColumn)
        );
        return pos >= from && pos <= to;
      });

      if (!match) {
        return null;
      }

      const from = positionFromLineColumn(view.state.doc, match.startLine, match.startColumn);
      const to = Math.max(
        from + 1,
        positionFromLineColumn(view.state.doc, match.endLine, match.endColumn)
      );

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
          kind.textContent = match.kind;
          head.append(kind);

          const title = document.createElement("div");
          title.className = "hematite-hover-title";
          title.textContent = match.title;
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
    { hoverTime: 260 }
  );
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
          editorTheme,
          syntaxHighlighting(editorHighlightStyle),
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
          hoverCompartment.of([]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              props.onChange(update.state.doc.toString());
            }
          }),
        ],
      }),
      parent: host,
    });

    view.dispatch(setDiagnostics(view.state, props.diagnostics));
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
    props.hoverItems;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: semanticCompartment.reconfigure(
        semanticDecorations(props.semanticTokens, props.hoverItems)
      ),
    });
  });

  createEffect(() => {
    props.hoverItems;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: hoverCompartment.reconfigure(hoverTooltips(props.hoverItems)),
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
