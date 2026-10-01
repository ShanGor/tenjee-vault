# Design: M2 笔记模块

## Context

M1 已交付：多库存储与迁移体系（`src-tauri/src/db/`，空间库 schema 含 `notebooks/section_groups/sections/pages/page_versions/attachments/taggings`，见 `migrations/space/0001_init.sql`）、加密原语（`src-tauri/src/crypto/`：`kdf.rs`/`keys.rs`/`cipher.rs`/`password_gen.rs`，`KdfParams`/`SectionKeys`/`WrappedDsk`/`seal`/`open` 接口）、启动完整性检查与 `VaultError` 错误体系。`src-tauri/src/commands/` 目前只有 `ping`/`db_status` 占位，`src-tauri/src/search/` 是空壳，`src/modules/notes/` 是占位文件，前端仅有最小 App.tsx。动机与范围见 proposal.md；行为契约见 `specs/notes-module/spec.md`。本设计只做 M2 的架构与技术选型决策。

## Goals / Non-Goals

**Goals:**
- 建立后端笔记领域层（空间注册表消费、层级 CRUD、页面读写、加密会话管理、FTS5 搜索）与对应 Tauri commands
- 建立前端笔记 UI：三栏导航、TipTap 编辑器、绘图块、附件、加密分区交互（设置/解锁/锁定/改密）、搜索视图
- 加密会话（密钥驻留、闲置自动锁定、内存索引销毁）的生命周期正确且可测试
- 复用 M1 全部原语，不改动 `crypto/` 与 `db/` 的既有接口

**Non-Goals:**
- 任务/日程业务（M3）；导入导出、备份、快捷键、托盘（M4）
- 多语言、主题切换之外的通用设置体系（M4 统一处理；本变更只为剪贴板清空与闲置时长提供最小设置存储）
- 命令面板、全局快捷键（M4）

## Decisions

### D1: 后端分层——`commands/` 薄层 + `notes/` 领域服务层 + `search/` 索引层

Tauri command 函数只做参数校验与错误映射（`VaultError` → 前端），业务逻辑放在 `src-tauri/src/notes/`（`hierarchy.rs` 层级 CRUD、`pages.rs` 页面读写与版本快照、`attachments.rs` 附件、`sections_crypto.rs` 加密分区设置/改密/移除、`session.rs` 解锁会话）与 `src-tauri/src/search/`（索引维护与查询）。command 保持纯函数可单测（不依赖 `tauri::AppHandle`，依赖注入 `AppState`）。

**备选**：把业务逻辑直接写进 command 函数（否决，M1 D3 已确立分层原则，否则无法单测）；引入 trait 抽象领域层（否决，M2 单一实现，过早抽象）。

### D2: 解锁会话——`SessionManager` 进程内单例，密钥材料集中持有与统一清零

`AppState` 内新增 `SessionManager`：`Mutex<HashMap<SectionId, UnlockedSection>>`，`UnlockedSection { keys: SectionKeys, last_activity: Instant }`。所有读写加密分区的路径必须先经 `SessionManager::get(section_id)` 取密钥，取不到即返回 `VaultError::SectionLocked`——锁定语义只有这一个收口，避免各处散判。闲置超时由前端心跳（编辑器/导航的 user-activity 事件每 30s 上报）+ 后端定时巡检（每 60s）双保险触发 `lock()`：从 map 移除（`SectionKeys` 的 `Zeroizing` Drop 清零内存）、通知前端、销毁内存索引（D4）。应用退出时在 Tauri `run` 的退出钩子统一调用 `lock_all()`。

**备选**：每个 command 各自持有密钥副本（否决，清零语义无法保证）；密钥放前端内存（否决，违反密钥不落盘的威胁模型）。

### D3: 加密分区的读写路径——「透明加解密」集中在数据访问层

`notes/pages.rs` 与 `notes/attachments.rs` 在读写 `content` 与附件字节时按 `sections.is_encrypted` 分派：普通分区直接读写明文；加密分区经 `SessionManager` 取 DSK 后 `seal`/`open`（格式沿用 M1 的版本字节 `v1 tag || ciphertext || nonce`）。密文统一存 `pages.content` BLOB/TEXT 列（密文以 base64 或二进制存储，迁移时定死为 BLOB 兼容写法）。设置/移除密码按 spec §修改密码 语义：设置时生成 DSK 并 wrap，既有明文页面**逐页惰性迁移**——设置密码即将该分区全部既有页面读出明文、`seal` 后写回（一次性批处理在事务内完成，页面量级可控）；移除密码反向解密写回。修改密码仅重包裹 DSK（`crypto::keys::wrap_dsk`），零数据重写。

**备选**：惰性迁移（下次编辑时才加密旧页面，否决——锁定前窗口期内旧页面仍是明文，违反「设置完成后分区锁定」的验收场景）；设置密码时拒绝非空分区（否决，用户体验差且 spec 明确要求支持既有页面）。

### D4: 搜索——空间库持久 FTS5 + 解锁分区内存临时 FTS 表

空间库迁移 `0002_fts.sql`：`pages_fts` FTS5 虚拟表（`title` + `content` 明文列）+ `AFTER INSERT/UPDATE/DELETE` 触发器同步。加密分区页面在明文状态下不写入 `pages_fts`（触发器内按 `sections.is_encrypted` 分派：加密分区仅写 `title`，正文列写空；或整体跳过正文——按 spec「锁定时完全不参与索引」采用：加密分区页面**完全不进入** `pages_fts`）。解锁时，`search/` 层在该空间库连接上 `CREATE TEMP TABLE pages_fts_unlocked_?<section_id>`（TEMP schema，随连接销毁）并对解锁分区页面解密正文重建临时索引；锁定即 `DROP`。查询时 `UNION` 持久表与各解锁分区的临时表。普通分区的全局搜索 SQL 只需一次 FTS 查询 + 内存表合并，应用层做上下文聚合与高亮片段提取。

**备选**：加密分区也建持久索引存密文（否决，FTS5 不支持可搜索加密，spec 明确锁定时不参与索引）；应用层内存倒排索引替代临时 FTS 表（否决，重复造轮子且无法复用 SQLite 的 snippet()/rank）。

### D5: 前端架构——zustand + React Router（或 TanStack Router）+ 手写轻量组件

- **状态管理**：`zustand`——空间/导航树/会话状态（解锁分区集合）/最近页面放 store，跨组件共享简单，避免 Context 层层透传。选择 zustand 而非 Redux Toolkit：体量小、无样板代码，与「纯本地单进程应用」的复杂度匹配。
- **路由**：笔记模块内部用 URL 状态（`space/:spaceId/notebook/.../page/:pageId`）深链可收藏；`react-router` 选型（v7）成熟稳定，TanStack Router 类型安全更好但生态较新，风险偏高。
- **UI 组件**：手写轻量组件（Tailwind 与否在 tasks 首任务中定，倾向 Tailwind 加速开发）。不引入重型组件库（Ant Design 等体积与设计语言均不契合离线小工具）。
- **编辑器**：`@tiptap/react` + StarterKit（标题/列表/链接）+ `@tiptap/extension-task-list/task-item`（复选框）+ `@tiptap/extension-table` 系列 + 自研 `DrawingBlock`（SVG）与 `AttachmentBlock` 节点；图片用 TipTap 的 Image 扩展，文件以附件块形式嵌入（区别于内联图片）。

**备选**：不使用路由、纯 store 驱动视图（否决，深链与浏览器式导航体验是笔记工具的硬需求）；Lexical 替代 TipTap（否决，TipTap 扩展生态对 task-list/table/placeholder 支持最完整且 spec.md 已决）。

### D6: 编辑器保存与版本快照——防抖自动保存 + 快照节流

编辑内容 1s 防抖自动保存到后端（transaction 内更新 `pages.content` + `updated_at`）；版本快照（`page_versions` 插入）与保存解耦：内容距上一快照变化超过阈值（默认 5 分钟或编辑器空闲时）才插快照，避免每次击键一条版本。回滚=把目标版本内容写入当前 `content` 并插一条新快照（spec 要求保留历史）。

### D7: 附件——内容寻址 + 引用计数

附件写入：计算 SHA-256 → 文件名 `<hash>`（加密分区先 `seal` 后对**密文**取哈希）→ 落盘 `<space_id>.files/`（哈希冲突即已存在，跳过写入）→ `attachments` 表登记。删除附件：引用计数归零才删物理文件（同哈希可能被多页面引用）。文件打开经 Tauri command 读取字节（加密分区取 DSK `open`）后交给前端/系统 opener。

### D8: 剪贴板清空与闲置时长——最小设置存储

沿用 `meta.db` 的 `app_config` 键值：`clipboard_auto_clear_seconds`（0=关闭）、`section_auto_lock_minutes`（默认 5）。从加密分区复制的文本由前端记录复制事件，定时经 Tauri 调 `tauri-plugin-clipboard-manager` 清空。默认空间名、锁定标题可见性（`encrypted_section_show_titles`，默认 true）同存于此。

## Risks / Trade-offs

- [解锁分区内存索引连接生命周期管理复杂（TEMP 表随连接消失）] → 每空间库连接由 `db/registry.rs` 常驻持有（M1 已确立每库单 Connection），TEMP 表生命周期与连接一致，锁定即 DROP 显式销毁；测试覆盖「解锁-搜索-锁定-再搜索」全序列
- [闲置超时双保险可能误判（前端心跳丢失导致提前锁定）] → 心跳仅作活性刷新，后端巡检为最终裁决；心跳间隔（30s）小于默认闲置时长（5min）一个数量级
- [设置/移除密码批量重写页面在大分区上的耗时] → 单事务内批处理 + 前端进度提示；极端大分区（>1 万页）的耗时在 tasks 中留基准测试项，超标再议分批方案
- [TipTap 依赖体积增大前端 bundle] → Vite 代码分割（编辑器路由懒加载）；离线场景首屏只加载导航壳
- [FTS5 中文分词质量] → FTS5 默认 tokenizer 对中文按单字切分，搜索可用但短语匹配弱；M2 接受单字分词（`unicode61`），jieba 分词插件留作 M4 优化项（不改 schema，仅换 tokenizer 需重建索引，属内部实现）
- [图片内联存 base64 撑爆页面 JSON] → 图片走附件管线（D7 内容寻址），文档内只存 `<hash>` 引用节点，渲染时按需加载

## Migration Plan

空间库新增 `0002_fts.sql`（FTS5 表 + 触发器 + 存量页面回填索引）——版本化迁移体系（M1 D2）自动应用，无需用户操作。无破坏性变更；回滚 = 删除数据目录（M2 数据均为新建，无既有用户数据）。前端新增依赖不影响已交付的 M1 功能。

## Open Questions

- 绘图块的笔刷参数（颜色/粗细预设集）在实现时与 UI 走查确定，不影响架构
- 图片「调整大小」的交互细节（拖拽手柄 vs 对话框输入）实现时定
- Tailwind 引入与否在 tasks 首个前端任务中结合 `src/App.css` 现状决定
