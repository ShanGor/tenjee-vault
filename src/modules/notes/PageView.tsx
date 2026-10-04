import { useParams } from "react-router-dom";
import { ui } from "../../i18n/ui";
import { Icon } from "../../shared/Icon";
import { useCompactLayout } from "../../shared/useCompactLayout";
import EditorPage from "./EditorPage";

export default function PageView() {
  const { spaceId = "", pageId } = useParams();
  const compact = useCompactLayout();
  return <section className="flex min-h-0 min-w-0 flex-1 flex-col">
    {pageId ? <EditorPage key={`${spaceId}:${pageId}`} /> : <div className="notes-welcome">
      <div className="welcome-symbol"><Icon name="notes" size={32} /></div>
      <p className="welcome-eyebrow">TENJEE VAULT</p>
      <h1>{ui("让想法有处安放")}</h1>
      <p className="welcome-description">{ui("在这里记录想法、整理计划，让重要的事井井有条。")}</p>
      <p className="welcome-hint">{ui(compact ? "打开上方的页面菜单，选择或新建页面开始书写。" : "从左侧选择或新建页面，开始书写。")}</p>
    </div>}
  </section>;
}
