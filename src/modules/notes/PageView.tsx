import { Component, type ReactNode } from "react";
import { useParams } from "react-router-dom";
import { ui, uiError } from "../../i18n/ui";
import { Icon } from "../../shared/Icon";
import { useCompactLayout } from "../../shared/useCompactLayout";
import EditorPage from "./EditorPage";

/** Keep a document/extension failure inside the current page, not the whole app. */
class PageErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: unknown) {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  render() {
    if (this.state.error) {
      return <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8" role="alert">
        <h1 className="text-xl font-semibold">{ui("页面显示失败")}</h1>
        <p className="max-w-xl break-words text-center text-sm text-neutral-500">{uiError(this.state.error)}</p>
        <p className="text-sm text-neutral-500">{ui("原始内容未被更改。可以重试，或切换到其他页面。")}</p>
        <button className="rounded border px-4 py-2" onClick={() => this.setState({ error: null })}>{ui("重试")}</button>
      </div>;
    }
    return this.props.children;
  }
}

export default function PageView() {
  const { spaceId = "", pageId } = useParams();
  const compact = useCompactLayout();
  return <section className="flex min-h-0 min-w-0 flex-1 flex-col">
    {pageId ? <PageErrorBoundary key={`${spaceId}:${pageId}`}><EditorPage /></PageErrorBoundary> : <div className="notes-welcome">
      <div className="welcome-symbol"><Icon name="notes" size={32} /></div>
      <p className="welcome-eyebrow">TENJEE VAULT</p>
      <h1>{ui("让想法有处安放")}</h1>
      <p className="welcome-description">{ui("在这里记录想法、整理计划，让重要的事井井有条。")}</p>
      <p className="welcome-hint">{ui(compact ? "打开上方的页面菜单，选择或新建页面开始书写。" : "从左侧选择或新建页面，开始书写。")}</p>
    </div>}
  </section>;
}
