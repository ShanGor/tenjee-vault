import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getMarkRange, type Editor } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import { closeHistory } from "@tiptap/pm/history";
import { ui } from "../../i18n/ui";
import { useMobileDismiss } from "../../shared/useMobileDismiss";

/** Resolve the whole link, including text split by bold/italic marks. */
export function selectedLink(editor: Editor) {
  const { selection, schema } = editor.state;
  if (!(selection instanceof TextSelection)) return null;
  const range = getMarkRange(selection.$from, schema.marks.link);
  if (!range || selection.to > range.to) return null;
  const mark = editor.state.doc.resolve(range.from).nodeAfter?.marks.find((item) => item.type === schema.marks.link);
  return mark ? { ...range, attrs: mark.attrs, href: String(mark.attrs.href) } : null;
}

export function removeEditorLink(editor: Editor, range: { from: number; to: number }) {
  editor.chain().command(({ tr }) => { closeHistory(tr); return true; })
    .setTextSelection(range).unsetLink().setTextSelection(range.to).focus().run();
}

function normalizeHref(value: string) {
  const url = value.trim();
  if (!url || /\s/.test(url)) return "";
  if (/^([a-z][\w+.-]*:|[#/?]|\.\.?\/)/i.test(url)) return url;
  if (/^[^@]+@[^@]+\.[^@]+$/.test(url)) return `mailto:${url}`;
  return `https://${url}`;
}

export function EditorLinkDialog({ editor, onClose }: { editor: Editor; onClose: () => void }) {
  const [target] = useState(() => {
    const link = selectedLink(editor);
    const { from, to } = link ?? editor.state.selection;
    return {
      from, to, link, bookmark: editor.state.selection.getBookmark(),
      text: editor.state.doc.textBetween(from, to, "\n"),
      marks: (from < to ? editor.state.doc.resolve(from).nodeAfter?.marks ?? [] : editor.state.storedMarks ?? editor.state.doc.resolve(from).marks())
        .filter((mark) => mark.type.name !== "link"),
    };
  });
  const [text, setText] = useState(target.text);
  const [href, setHref] = useState(target.link?.href ?? "");
  const [error, setError] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const urlInput = useRef<HTMLInputElement>(null);
  const textInput = useRef<HTMLInputElement>(null);
  const id = useId();
  const title = ui(target.link ? "编辑链接" : "插入链接");

  function cancel() {
    editor.chain().command(({ tr }) => { tr.setSelection(target.bookmark.resolve(tr.doc)); return true; }).focus().run();
    onClose();
  }
  const cancelRef = useRef(cancel);
  cancelRef.current = cancel;
  useMobileDismiss(cancel);

  useEffect(() => {
    (target.link ? textInput : urlInput).current?.focus();
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancelRef.current(); }
      if (event.key !== "Tab") return;
      const elements = Array.from(panel.current?.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)") ?? []);
      const first = elements[0], last = elements[elements.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    document.addEventListener("keydown", keyboard, true);
    return () => document.removeEventListener("keydown", keyboard, true);
  }, [target]);

  function apply() {
    const url = normalizeHref(href);
    let valid = !!url && editor.can().setLink({ href: url });
    if (/^https?:/i.test(url)) {
      try { valid = valid && !!new URL(url).hostname; } catch { valid = false; }
    }
    if (!valid) { setError(true); urlInput.current?.focus(); return; }
    const label = text || url;
    const command = editor.chain().command(({ tr }) => { closeHistory(tr); return true; })
      .setTextSelection({ from: target.from, to: target.to });
    // Updating only the URL keeps every existing text style and inline node.
    if (label === target.text) command.setLink({ ...target.link?.attrs, href: url });
    else command.insertContentAt({ from: target.from, to: target.to }, {
      type: "text", text: label,
      marks: [...target.marks.map((mark) => mark.toJSON()), { type: "link", attrs: { ...target.link?.attrs, href: url } }],
    });
    const end = label === target.text ? target.to : target.from + label.length;
    command.setTextSelection(end).unsetMark("link").setMeta("preventAutolink", true).focus().run();
    onClose();
  }

  return createPortal(<div className="editor-link-overlay" onClick={cancel}>
    <section ref={panel} className="editor-link-dialog" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} onClick={(event) => event.stopPropagation()}>
      <header><h2 id={`${id}-title`}>{title}</h2><button type="button" aria-label={ui("关闭")} onClick={cancel}>×</button></header>
      <form noValidate onSubmit={(event) => { event.preventDefault(); apply(); }}>
        <label htmlFor={`${id}-text`}>{ui("显示文字")}</label>
        <input ref={textInput} id={`${id}-text`} value={text} placeholder={ui("留空则显示链接地址")} onChange={(event) => setText(event.target.value)} />
        <label htmlFor={`${id}-url`}>{ui("链接地址")}</label>
        <input ref={urlInput} id={`${id}-url`} value={href} inputMode="url" autoComplete="off" autoCapitalize="none" spellCheck={false} placeholder="https://example.com"
          aria-invalid={error} aria-describedby={error ? `${id}-error` : `${id}-hint`} onChange={(event) => { setHref(event.target.value); setError(false); }} />
        {error ? <p id={`${id}-error`} role="alert" className="editor-link-error">{ui("请输入有效的链接地址")}</p>
          : <p id={`${id}-hint`} className="editor-link-hint">{ui("可输入网址或邮箱，网址会自动补全 https://")}</p>}
        <footer>
          {target.link && <button type="button" className="editor-link-unlink" onClick={() => {
            removeEditorLink(editor, target); onClose();
          }}>{ui("取消链接")}</button>}
          <button type="button" onClick={cancel}>{ui("取消")}</button>
          <button type="submit" className="editor-link-save">{ui(target.link ? "保存" : "插入链接")}</button>
        </footer>
      </form>
    </section>
  </div>, document.body);
}
