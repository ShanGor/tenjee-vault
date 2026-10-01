# Proposal: M2 笔记模块（组织结构 + 编辑器 + 加密分区 + 绘图 + 搜索）

## Why

M1 已完成技术地基：Tauri 2 + React 19 骨架、多库分离存储（空间库的 `notebooks/section_groups/sections/pages/page_versions/attachments/taggings` 表结构已就位）、加密原语（Argon2id KDF、AES-256-GCM、DSK/KEK 两层包裹、验证器、密码生成器）均已实现并有测试覆盖。但应用目前没有任何笔记业务功能——模块目录是空占位，notes UI 完全缺失。M2 是产品的核心价值里程碑：按 `spec.md` §4.3 交付完整的笔记模块（空间/组织结构、TipTap 编辑器、SVG 绘图、加密分区全生命周期、全文搜索），让用户真正能开始写笔记并安全存放敏感信息。

## What Changes

- **笔记组织结构**：空间（新建/重命名/归档/删除）、笔记本、可嵌套分区组、分区、多级页面的完整 CRUD、重命名、拖拽移动与排序、颜色标记；前端三栏式导航（空间/笔记本列表 → 分区 → 页面树）
- **TipTap 富文本编辑器**：流式文档，支持标题层级、粗斜体/下划线/删除线/高亮、字体颜色字号、项目符号/编号/待办复选框列表、表格、图片（粘贴/拖入/调整大小）、页面间双向链接 `[[页面名]]`、外部链接；内容以 TipTap JSON 存入 `pages.content`
- **SVG 绘图模式**：页面内插入绘图块，笔画以 SVG 存储并嵌入页面内容
- **文件附件**：任意文件嵌入页面，哈希命名存入 `<space_id>.files/` 目录；加密分区的附件内容加密存储
- **加密分区全生命周期**：设置/移除分区密码（复用 M1 的 wrap/unwrap 原语）、解锁/锁定、闲置 N 分钟（默认 5）自动锁定、退出应用自动锁定；锁定时内容不可见、不参与搜索；修改密码仅重包裹 DSK（不重加密数据）；忘记密码不可恢复，设置时强制确认提示；内置密码生成器 UI 与剪贴板 30 秒自动清空（可选设置）
- **笔记搜索**：各空间库 FTS5 索引（页面标题 + 正文）；加密分区解锁时建立内存临时 FTS 表、锁定即销毁不落盘；全局搜索聚合、按笔记本/分区过滤、关键词高亮
- **页面管理**：自动版本快照（可查看/回滚）、回收站（恢复/彻底删除）、最近使用页面列表
- **后端**：`src-tauri/src/notes/`（领域服务层）、`src-tauri/src/search/` 的 FTS5 实现、`src-tauri/src/commands/` 的笔记相关 Tauri commands、空间库的 FTS5 迁移（`0002_fts.sql`）
- **前端基建**：引入 TipTap、zustand（或等价轻量状态管理）、前端路由与基础 UI 组件体系（M1 D6 遗留的选型问题在本变更内决定）

非目标（本变更不做）：任务/日程任何业务功能（M3）；导入导出 Markdown/HTML/PDF、打印（M4）；OneNote 式自由画布、录音、页面模板（v1.1+）；`[[页面名]]` 反向链接面板与标签汇总视图（M4 通用功能）。

## Capabilities

### New Capabilities

- `notes-module`: 笔记模块的完整行为规格——空间/笔记本/分区组/分区/页面的组织与 CRUD、TipTap 流式文档编辑器与内容存储、SVG 绘图块、附件、加密分区的设置/解锁/锁定/自动锁定/改密全生命周期（含锁定时对搜索的隔离）、页面版本历史与回收站、基于 FTS5 的笔记全文搜索（含加密分区的内存临时索引）

### Modified Capabilities

（无——`crypto-core` 与 `data-storage` 的需求在 M1 已定型，本变更仅消费其能力；新增的 FTS5 迁移属于 `data-storage` 既有迁移体系的常规扩展，不改变其需求。）

## Impact

- **代码**：新建/扩展 `src-tauri/src/notes/`、`src-tauri/src/search/`、`src-tauri/src/commands/`；重写 `src/modules/notes/`（当前为占位文件），扩展 `src/shared/`（组件、状态、搜索 UI）
- **依赖**：前端新增 `@tiptap/*`（core、react、starter-kit、table、task-list、link、placeholder 等）、`zustand`、前端路由；Rust 新增 `rusqlite` 的 FTS5 相关使用（bundled 已含）、可能新增 `base64`/`mime_guess`；版本快照/附件哈希的既有 schema 直接复用
- **数据**：空间库新增 FTS5 虚拟表与触发器（`0002_fts.sql` 迁移）；附件首次写入 `<space_id>.files/` 目录；加密分区用户首次设置密码后 `sections` 表的加密字段被填充
- **兼容**：全部为新增能力，无破坏性变更；密文格式沿用 M1 的版本字节约定（v1）
- **风险**：TipTap 前端依赖体积与编辑器性能；加密分区解锁后的内存索引生命周期（泄漏 = 敏感内容滞留内存）需在测试中重点验证；闲置自动锁定跨窗口/跨命令的一致性
