import { ui, localizeElement } from "../../i18n/ui";
// TipTap 自定义节点扩展：
// - AttachmentImage：图片走附件内容寻址存储，文档内只存 hash 引用节点（spec 5.3）
// - PageLink：`[[页面名]]` 双向链接节点（spec 5.4）
// - DrawingBlock：SVG 绘图块（spec 5.5）
// - AttachmentBlock：文件附件块（spec 5.6）

import Image from "@tiptap/extension-image";
import { mergeAttributes, Node } from "@tiptap/core";
import { base64ToBytes, bytesToBase64 } from "./api";

/** 附件字节 → blob URL 的进程内缓存（同哈希只解一次）。 */
const blobUrlCache = new Map<string, string>();

export async function attachmentUrl(
  openAttachment: (id: string) => Promise<{ data_base64: string; mime: string | null; file_name: string }>,
  attachmentId: string,
): Promise<string> {
  const cached = blobUrlCache.get(attachmentId);
  if (cached) return cached;
  const data = await openAttachment(attachmentId);
  const bytes = base64ToBytes(data.data_base64);
  const url = URL.createObjectURL(new Blob([bytes], { type: data.mime ?? "application/octet-stream" }));
  blobUrlCache.set(attachmentId, url);
  return url;
}

export function revokeAttachmentUrls() {
  for (const url of blobUrlCache.values()) URL.revokeObjectURL(url);
  blobUrlCache.clear();
}

type OpenAttachmentFn = (id: string) => Promise<{ data_base64: string; mime: string | null; file_name: string }>;

/** 图片：src 存 `attachment://<id>`，渲染时经后端取字节，支持右下角拖拽调整大小。 */
export function AttachmentImage(openAttachment: OpenAttachmentFn) {
  return Image.extend({
    addAttributes() {
      return {
        ...this.parent?.(),
        attachmentId: {
          default: null,
          parseHTML: (el) => el.getAttribute("data-attachment-id"),
          renderHTML: (attrs) => ({ "data-attachment-id": attrs.attachmentId }),
        },
        width: {
          default: null,
          parseHTML: (el) => el.getAttribute("data-width"),
          renderHTML: (attrs) => (attrs.width ? { "data-width": attrs.width } : {}),
        },
      };
    },

    parseHTML() {
      return [{ tag: "img[data-attachment-id]" }];
    },

    addNodeView() {
      return ({ node, editor, getPos }) => {
        const dom = document.createElement("div");
        dom.className = "attachment-image";
        const img = document.createElement("img");
        img.draggable = false;
        img.style.maxWidth = "100%";
        img.style.display = "block";
        if (node.attrs.width) img.style.width = node.attrs.width;
        dom.appendChild(img);

        const id = node.attrs.attachmentId as string | null;
        if (id) {
          attachmentUrl(openAttachment, id)
            .then((url) => {
              img.src = url;
            })
            .catch(() => {
              localizeElement(img, "（图片不可读：附件缺失或分区已锁定）", "alt");
            });
        }

        // 调整大小：右下角拖拽手柄
        const handle = document.createElement("span");
        handle.className = "resize-handle";
        handle.contentEditable = "false";
        dom.appendChild(handle);
        handle.addEventListener("mousedown", (e) => {
          e.preventDefault();
          const startX = e.clientX;
          const startW = img.getBoundingClientRect().width;
          const onMove = (ev: MouseEvent) => {
            const w = Math.max(60, startW + ev.clientX - startX);
            img.style.width = `${w}px`;
          };
          const onUp = (ev: MouseEvent) => {
            window.removeEventListener("mousemove", onMove);
            window.removeEventListener("mouseup", onUp);
            if (typeof getPos === "function" && !editor.isDestroyed) {
              const pos = getPos() ?? 0;
              const w = Math.max(60, startW + ev.clientX - startX);
              editor
                .chain()
                .focus()
                .command(({ tr, dispatch }) => {
                  if (dispatch) tr.setNodeMarkup(pos, undefined, { ...node.attrs, width: `${w}px` });
                  return true;
                })
                .run();
            }
          };
          window.addEventListener("mousemove", onMove);
          window.addEventListener("mouseup", onUp);
        });
        return { dom, contentDOM: undefined };
      };
    },
  });
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    pageLink: {
      insertPageLink: (attrs: { pageId: string; label: string }) => ReturnType;
    };
    attachmentBlock: {
      insertAttachmentBlock: (attrs: { attachmentId: string; fileName: string; size: number }) => ReturnType;
    };
    drawingBlock: {
      insertDrawingBlock: (attrs?: { svg?: string }) => ReturnType;
      setDrawingSvg: (svg: string) => ReturnType;
    };
  }
}

/** `[[页面名]]` 双向链接：点击跳转（spec 5.4）。 */
export const PageLink = Node.create({
  name: "pageLink",
  group: "inline",
  inline: true,
  atom: true,

  addAttributes() {
    return {
      pageId: { default: null },
      label: { default: "" },
    };
  },

  parseHTML() {
    return [{ tag: "a[data-page-link]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["a", mergeAttributes(HTMLAttributes, { "data-page-link": "true", class: "page-link" }), 0];
  },

  addCommands() {
    return {
      insertPageLink:
        (attrs) =>
        ({ chain }) =>
          chain().insertContent({ type: this.name, attrs }).run(),
    };
  },
});

/** 文件附件块：打开 / 删除（联动后端引用计数，spec 5.6）。 */
export function AttachmentBlock(deps: {
  openAttachment: OpenAttachmentFn;
  deleteAttachment: (id: string) => Promise<void>;
}) {
  return Node.create({
    name: "attachmentBlock",
    group: "block",
    atom: true,

    addAttributes() {
      return {
        attachmentId: { default: null },
        fileName: { default: ui("附件") },
        size: { default: 0 },
      };
    },

    parseHTML() {
      return [{ tag: "div[data-attachment-block]" }];
    },

    renderHTML({ HTMLAttributes }) {
      return ["div", mergeAttributes(HTMLAttributes, { "data-attachment-block": "true" }), 0];
    },

    addCommands() {
      return {
        insertAttachmentBlock:
          (attrs) =>
          ({ chain }) =>
            chain().insertContent({ type: this.name, attrs }).run(),
      };
    },

    addNodeView() {
      return ({ node, editor, getPos }) => {
        const dom = document.createElement("div");
        dom.className = "attachment-block";
        dom.setAttribute("data-attachment-block", "true");
        const name = document.createElement("span");
        name.className = "attachment-name";
        name.textContent = `📎 ${node.attrs.fileName} (${formatSize(node.attrs.size)})`;
        const openBtn = document.createElement("button");
        localizeElement(openBtn, "打开");
        openBtn.type = "button";
        openBtn.onclick = async () => {
          const data = await deps.openAttachment(node.attrs.attachmentId as string);
          const bytes = base64ToBytes(data.data_base64);
          const url = URL.createObjectURL(new Blob([bytes], { type: data.mime ?? "application/octet-stream" }));
          window.open(url, "_blank");
        };
        const delBtn = document.createElement("button");
        localizeElement(delBtn, "删除");
        delBtn.type = "button";
        delBtn.onclick = async () => {
          if (!editor.isEditable) return;
          if (!window.confirm(ui("删除附件 {p0}？", { p0: String(node.attrs.fileName) }))) return;
          await deps.deleteAttachment(node.attrs.attachmentId as string);
          if (typeof getPos === "function" && !editor.isDestroyed) {
            const pos = getPos() ?? 0;
            editor.chain().focus().deleteRange({ from: pos, to: pos + 1 }).run();
          }
        };
        const updateEditable = () => { delBtn.hidden = !editor.isEditable; };
        updateEditable();
        editor.on("update", updateEditable);
        dom.append(name, openBtn, delBtn);
        return { dom, destroy: () => editor.off("update", updateEditable) };
      };
    },
  });
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

/** SVG 绘图块：笔画采集 → SVG 序列化嵌入文档 → 可再编辑/删除（spec 5.5）。 */
export const DrawingBlock = Node.create({
  name: "drawingBlock",
  group: "block",
  atom: true,

  addAttributes() {
    return {
      svg: { default: "" },
    };
  },

  parseHTML() {
    return [{ tag: "div[data-drawing-block]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["div", mergeAttributes(HTMLAttributes, { "data-drawing-block": "true" }), 0];
  },

  addCommands() {
    return {
      insertDrawingBlock:
        (attrs = {}) =>
        ({ chain }) =>
          chain().insertContent({ type: this.name, attrs }).run(),
      setDrawingSvg:
        (svg) =>
        ({ commands }) =>
          commands.updateAttributes(this.name, { svg }),
    };
  },

  addNodeView() {
    return ({ node, editor, getPos }) => {
      const dom = document.createElement("div");
      dom.className = "drawing-block";
      dom.setAttribute("data-drawing-block", "true");
      let editing = !(node.attrs.svg as string);

      const render = () => {
        dom.innerHTML = "";
        if (!editor.isEditable || (!editing && node.attrs.svg)) {
          const view = document.createElement("div");
          view.className = "drawing-view";
          view.innerHTML = node.attrs.svg as string;
          view.addEventListener("click", () => {
            if (!editor.isEditable) return;
            editing = true;
            render();
          });
          const hint = document.createElement("span");
          hint.className = "drawing-hint";
          localizeElement(hint, "点击继续编辑");
          dom.append(view);
          if (editor.isEditable) dom.append(hint);
          return;
        }

        // 编辑态：画布采集笔画
        const toolbar = document.createElement("div");
        toolbar.className = "drawing-toolbar";
        const color = document.createElement("input");
        color.type = "color";
        color.value = "#1f2937";
        const width = document.createElement("select");
        for (const [label, value] of [
          ["细", "2"],
          ["中", "4"],
          ["粗", "7"],
        ] as const) {
          const opt = document.createElement("option");
          opt.value = value;
          localizeElement(opt,label);
          width.appendChild(opt);
        }
        const doneBtn = document.createElement("button");
        doneBtn.type = "button";
        localizeElement(doneBtn, "完成");
        doneBtn.setAttribute("data-save-drawing", "true");
        const clearBtn = document.createElement("button");
        clearBtn.type = "button";
        localizeElement(clearBtn, "清空");
        const delBtn = document.createElement("button");
        delBtn.type = "button";
        localizeElement(delBtn, "删除块");
        toolbar.append(color, width, doneBtn, clearBtn, delBtn);
        dom.appendChild(toolbar);

        const svgNS = "http://www.w3.org/2000/svg";
        const svg = document.createElementNS(svgNS, "svg");
        svg.setAttribute("width", "100%");
        svg.setAttribute("height", "220");
        svg.setAttribute("viewBox", "0 0 640 220");
        svg.classList.add("drawing-canvas");
        dom.appendChild(svg);

        // 既有笔画回显（继续编辑）
        if (node.attrs.svg) {
          const tmp = document.createElement("div");
          tmp.innerHTML = node.attrs.svg as string;
          const oldSvg = tmp.querySelector("svg");
          if (oldSvg) {
            for (const child of Array.from(oldSvg.childNodes)) {
              svg.appendChild(document.importNode(child, true));
            }
          }
        }

        let drawing = false;
        let points: string[] = [];
        let current: SVGPolylineElement | null = null;

        const toPoint = (e: MouseEvent) => {
          const rect = svg.getBoundingClientRect();
          const x = ((e.clientX - rect.left) / rect.width) * 640;
          const y = ((e.clientY - rect.top) / rect.height) * 220;
          return `${x.toFixed(1)},${y.toFixed(1)}`;
        };
        svg.addEventListener("mousedown", (e) => {
          drawing = true;
          points = [toPoint(e)];
          current = document.createElementNS(svgNS, "polyline");
          current.setAttribute("fill", "none");
          current.setAttribute("stroke", color.value);
          current.setAttribute("stroke-width", width.value);
          current.setAttribute("stroke-linecap", "round");
          current.setAttribute("stroke-linejoin", "round");
          current.setAttribute("points", points.join(" "));
          svg.appendChild(current);
        });
        svg.addEventListener("mousemove", (e) => {
          if (!drawing || !current) return;
          points.push(toPoint(e));
          current.setAttribute("points", points.join(" "));
        });
        window.addEventListener("mouseup", () => {
          drawing = false;
          current = null;
        });

        doneBtn.onclick = () => {
          if (!editor.isEditable) return;
          const serializer = new XMLSerializer();
          const svgText = serializer.serializeToString(svg);
          if (typeof getPos === "function" && !editor.isDestroyed) {
            const pos = getPos() ?? 0;
            editor
              .chain()
              .focus()
              .command(({ tr, dispatch }) => {
                if (dispatch) tr.setNodeMarkup(pos, undefined, { svg: svgText });
                return true;
              })
              .run();
          }
          editing = false;
          render();
        };
        clearBtn.onclick = () => {
          if (!editor.isEditable) return;
          for (const child of Array.from(svg.childNodes)) child.remove();
        };
        delBtn.onclick = () => {
          if (!editor.isEditable) return;
          if (typeof getPos === "function" && !editor.isDestroyed) {
            const pos = getPos() ?? 0;
            editor.chain().focus().deleteRange({ from: pos, to: pos + 1 }).run();
          }
        };
      };

      let editable = editor.isEditable;
      const updateEditable = () => {
        if (editable === editor.isEditable) return;
        editable = editor.isEditable;
        render();
      };
      editor.on("update", updateEditable);
      render();
      return {
        dom,
        destroy: () => editor.off("update", updateEditable),
        // 属性更新（如回滚后）时重渲染
        update(updated) {
          if (updated.type.name !== "drawingBlock") return false;
          Object.assign(node.attrs, updated.attrs);
          editing = !(updated.attrs.svg as string);
          render();
          return true;
        },
      };
    };
  },
});

export { bytesToBase64 };
