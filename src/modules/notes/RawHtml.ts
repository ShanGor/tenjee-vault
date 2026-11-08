import { Node } from "@tiptap/core";
import { Marked } from "marked";

// Use a separate lexer so these tokenizers never recursively invoke themselves.
const lexer = new Marked({ gfm: true });
const voidTags = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);
const editableTags = new Set(["p", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "b", "em", "i", "s", "del", "u", "code", "pre", "blockquote", "ul", "ol", "li", "br", "hr", "a", "img", "table", "thead", "tbody", "tr", "td", "th", "details", "summary"]);

function needsRawHtml(html: string): boolean {
  const document = new DOMParser().parseFromString(html, "text/html");
  return [...document.querySelectorAll("*")].some((element) => {
    if (["html", "head", "body"].includes(element.localName)) return false;
    if (!editableTags.has(element.localName)) return true;
    return [...element.attributes].some(({ name }) =>
      !["href", "src", "alt", "title", "colspan", "rowspan", "align", "open"].includes(name));
  });
}

function inlineHtml(source: string): string | undefined {
  const tokens = lexer.Lexer.lexInline(source, { gfm: true });
  if (tokens[0]?.type !== "html") return;
  const first = tokens[0].raw;
  const opening = /^<([\w-]+)(?:\s|>|\/)/.exec(first);
  if (!opening || voidTags.has(opening[1].toLowerCase()) || /\/>$/.test(first)) return first;
  let depth = 0;
  let html = "";
  for (const token of tokens) {
    html += token.raw;
    if (token.type !== "html") continue;
    const tag = /^<(\/?)\s*([\w-]+)\b/.exec(token.raw);
    if (tag?.[2].toLowerCase() !== opening[1].toLowerCase()) continue;
    depth += tag[1] ? -1 : /\/>$/.test(token.raw) ? 0 : 1;
    if (depth === 0) return html;
  }
  return first;
}

function rawHtmlNode(inline: boolean) {
  const name = inline ? "rawHtmlInline" : "rawHtmlBlock";
  return Node.create({
    name,
    group: inline ? "inline" : "block",
    inline,
    atom: true,
    addAttributes() {
      return { html: { default: "", parseHTML: (element) => element.getAttribute("data-raw-html") ?? "", rendered: false } };
    },
    parseHTML() { return [{ tag: `${inline ? "span" : "div"}[data-raw-html]` }]; },
    renderHTML({ node }) {
      const element = document.createElement(inline ? "span" : "div");
      element.className = "note-raw-html";
      element.setAttribute("data-raw-html", node.attrs.html);
      element.innerHTML = node.attrs.html;
      return element;
    },
    markdownTokenName: name,
    markdownTokenizer: {
      name,
      level: inline ? "inline" : "block",
      start(source) {
        if (inline) return source.indexOf("<");
        // Block hints must not interrupt a paragraph at an inline HTML tag.
        for (const candidate of source.matchAll(/^ {0,3}(?=<)/gm)) {
          const token = lexer.lexer(source.slice(candidate.index))[0];
          if (token?.type === "html" && needsRawHtml(token.raw)) return candidate.index;
        }
        return -1;
      },
      tokenize(source) {
        if (!source.startsWith("<")) return;
        const token = inline ? undefined : lexer.lexer(source)[0];
        let raw = inline ? inlineHtml(source) : token?.type === "html" ? token.raw : undefined;
        // CommonMark ends some HTML blocks at a blank line. Keep a complete
        // enclosing element together so its layout and native controls survive.
        if (!inline && raw) {
          const complete = inlineHtml(source);
          if (complete && complete.length > raw.length) raw = complete;
        }
        if (!raw || !needsRawHtml(raw)) return;
        return { type: name, raw, text: raw };
      },
    },
    parseMarkdown(token) { return { type: name, attrs: { html: token.text ?? token.raw } }; },
    renderMarkdown(node) { return String(node.attrs?.html ?? ""); },
  });
}

// Raw HTML is trusted personal note content, just like HTML in Trinity.
export const RawHtmlBlock = rawHtmlNode(false);
export const RawHtmlInline = rawHtmlNode(true);
