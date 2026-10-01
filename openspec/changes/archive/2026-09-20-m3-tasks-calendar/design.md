# Design: M3 任务模块 + 日程模块（含农历）

## Context

M1/M2 已交付：多库存储与迁移体系（`src-tauri/src/db/`，`DbKind::Tasks`/`DbKind::Calendar` 迁移链已注册，`tasks.db`/`calendar.db` 初始 schema 含 `task_lists`/`tasks`/`events`/`event_exceptions`）、加密原语、笔记领域层（`src-tauri/src/notes/`）、FTS5 搜索层（`src-tauri/src/search/`，`tv_seg` CJK 单字切分函数在每个连接上注册）、薄 commands 层与 `AppStateInner`（`meta: Mutex<Connection>` + `spaces: Mutex<HashMap<_, Connection>>`）。前端：React 19 + zustand + react-router + Tailwind 4 + TipTap，笔记模块已建立 AppShell/store/api 分层模式，但 `src/App.tsx` 直接渲染 NotesApp，无模块级导航；`src/modules/tasks|calendar/` 为占位文件。动机与范围见 proposal.md；行为契约见 `specs/tasks-module/spec.md` 与 `specs/calendar-module/spec.md`。本设计只做 M3 的架构与技术选型决策。

## Goals / Non-Goals

**Goals:**
- 建立后端任务/日程领域层（`tasks/`、`calendar/` 含 `lunar.rs` 与 `rrule.rs`、`remind/` 提醒调度）与对应 Tauri commands，沿用 M2 的薄 command + 领域服务分层
- 建立前端应用外壳（模块导航）与任务/日程模块 UI：任务列表/看板/智能视图，日历月/周/日/议程视图，拖拽交互，农历叠加
- 重复规则展开、农历换算、提醒调度的正确性可单测（不依赖 WebView）
- 复用 M1/M2 全部基建（迁移体系、`tv_seg`、设置存储、错误体系），不改动 `crypto/`、`db/`、`notes/` 的既有接口

**Non-Goals:**
- `.ics` 导入导出（M4）；任务快速添加的自然语言解析（M4 打磨）；命令面板/全局搜索 UI（M4）
- 完整 RFC 5545 全集（BYSETPOS、BYWEEKNO、EXDATE 语法等）；时区换算与多时区显示（v1 存字段、按本地时区显示）
- 笔记编辑器内部改动（待办复选框与任务打通属 M4）

## Decisions

### D1: 后端分层与连接管理——沿用 M2 模式，领域库常驻单连接

`AppStateInner` 新增 `tasks: Mutex<Connection>` 与 `calendar: Mutex<Connection>`，在 `AppState::init` 中与主库一同打开并迁移（领域库按 data-storage 布局必然存在，无需像空间库那样惰性打开）。新增 `with_tasks`/`with_calendar` 访问器。业务逻辑放 `src-tauri/src/tasks/`（`lists.rs`、`tasks.rs`、`attachments.rs`）与 `src-tauri/src/calendar/`（`events.rs`、`rrule.rs`、`lunar.rs`、`festivals.rs`），commands 保持薄层纯函数可单测。

**备选**：领域库惰性打开（否决，两个领域库必然使用，惰性打开只增复杂度）；tasks/calendar 共用一个模块目录（否决，两个能力边界清晰，spec 也分为两个 capability）。

### D2: 重复规则——Rust 自研 RFC 5545 有界子集，前后端统一以后端展开为准

`calendar/rrule.rs` 实现有界子集：`FREQ=DAILY|WEEKLY|MONTHLY|YEARLY`、`INTERVAL`、`COUNT`、`UNTIL`、`BYDAY`（周）、`BYMONTHDAY`（月）。数据库 `recurrence_rule` 存原始 RRULE 字符串（为 M4 的 `.ics` 导出保留往返能力）；解析失败或含不支持部件时返回校验错误，不静默忽略。所有展开计算在 Rust 端：前端请求可见范围内的事件实例，后端展开重复事件并应用例外后返回。任务的 `recurrence_rule` 复用同一模块：完成时按规则从当前截止日推算下一截止日生成新实例。

**备选**：引入 `rrule` crate（否决，维护状态不明且功能远超所需，核心调度逻辑受制于外部依赖）；前端 rrule.js 展开（否决，提醒调度在后端，展开逻辑必须 Rust 侧存在，双实现必不一致）；物化实例表（否决，见 D3）。

### D3: 重复事件存储——规则 + 例外表，查询时展开，不物化实例

沿用 M1 schema：`events.recurrence_rule` + `event_exceptions(event_id, original_start_at, new_start_at, is_cancelled)`。视图查询流程：范围内普通事件直接命中；重复事件按 RRULE 在范围内展开，再应用例外（取消的剔除、改期的替换）。实例身份 = `event_id + original_start_at`。修改单次 → 插入例外行；修改全部 → 更新事件本体；删除单次 → 插入 `is_cancelled` 例外；删除全部 → 删除事件（例外行级联删除）。单一事实来源，无一致性问题；范围展开代价与可见范围成正比，可接受。

**备选**：物化全部实例到表（否决，无限重复无法物化，且修改规则需批量重写）；懒物化已过去实例（否决，双数据源带来一致性负担）。

### D4: 农历与节气——内置 1900–2100 查表，纯本地纯索引计算

`calendar/lunar.rs` 内置经典 200 年位掩码农历表（每年一个 u32 编码各月大小与闰月月份），提供：公历→农历日期、农历（月/日/是否闰月）→公历日期、某年闰月查询。`calendar/festivals.rs`：传统节日由农历日期推导（春节=正月初一、除夕=腊月最后一日、元宵、端午、七夕、中元、中秋、重阳、腊八等）；二十四节气用内置查表（每年 24 个节气日的紧凑表，与农历表同源生成）。正确性以权威日期对照做单测（如 2026-02-17=正月初一/春节、多年份清明/中秋/冬至）。换算结果按公历日 memoize 缓存（月视图每格都要算）。

农历重复（`events.lunar_recurrence` JSON `{month, day, leap_month: "ignore"|"only"}`）展开：对范围内每个农历年做农历→公历换算；`ignore` 时闰月年份按平月，`only` 时仅含该闰月的年份出现。

**备选**：天文算法实时计算节气与朔望月（否决，复杂且易错，spec 明确允许内置查表）；引入农历 crate（否决，核心功能受制于外部依赖的维护与数据准确性）；前端换算（否决，农历重复事件的展开与提醒必须在后端）。

### D5: 多提醒 schema 扩展——`event_reminders` 表，迁移既有数据后废弃旧列

`calendar.db` 迁移 `0002_reminders_fts.sql`：新建 `event_reminders(id, event_id, minutes_before)`（一对多，spec §4.1 要求多个提前量）；将既有 `events.reminder_minutes` 非空值迁入新表后 `DROP COLUMN reminder_minutes`（rusqlite bundled 的 SQLite 版本支持；pre-1.0 无真实用户数据，但仍写数据迁移 SQL 保证正确）。`tasks.db` 迁移 `0002_fts_archive.sql`：新增 `tasks.archived_at TEXT NULL` 列（M1 schema 缺归档字段，spec §4.2 要求归档/恢复）；任务提醒保持单 `reminder_at` 绝对时间（spec 未要求多提醒）。

**备选**：提醒存 JSON 数组列（否决，无法对提醒时间建索引，调度扫描需全表解析）；保留 `reminder_minutes` 双写（否决，双数据源）。

### D6: 提醒调度——后台巡检线程 + 持久化已发记录 + 启动补发

新建 `src-tauri/src/remind/`：后台线程每 30s 巡检（与 M2 闲置锁定巡检线程并存），扫描 `tasks.reminder_at` 与近水平线内的事件实例（重复事件按 D3 展开）× `event_reminders` 提前量，到期经 `tauri-plugin-notification` 发系统通知（线程持有 `AppHandle`，与 M2 无事件通道的巡检线程不同）。已发记录持久化到 `meta.db` 新表 `reminder_fires(entity_kind, entity_id, occurrence_key, slot, fired_at)`（0003 迁移），防重启重复发送。启动补发：应用启动时对停机期间到期且仍相关的提醒补发（任务未完成；事件实例未结束），超量（>20 条）合并为一条汇总通知。启动时请求通知权限，失败降级为应用内提示。

**备选**：为每个提醒设定时器（否决，重复事件实例无限且应用重启后全部丢失）；已发记录只存内存（否决，重启后同一提醒重发）；系统级定时唤醒（否决，跨平台复杂度高，30s 巡检对桌面应用足够）。

### D7: 任务/日程 FTS5——复用 `tv_seg` 与触发器模式，查询层过滤归档

`tasks.db` 0002 迁移建 `tasks_fts(title, notes)` + 增删改触发器（索引用 `tv_seg` 分词，与 M2 空间库一致）；`calendar.db` 0002 迁移建 `events_fts(title, description)` + 触发器。rowid 约定同 M2（FTS rowid == 表 rowid）。归档任务不特殊处理触发器，查询层默认 `WHERE archived_at IS NULL`（选择包含归档时放开）。重复事件搜索命中返回事件本体（按 `events.id` 去重）。`search/` 层新增 tasks/calendar 查询函数，复用 `segment_cjk` 与 snippet 提取。

**备选**：归档任务从索引剔除（否决，触发器需感知归档字段，恢复归档还要回填，查询层过滤零成本）；全局聚合搜索 UI（否决，M4 命令面板范畴；本变更只提供后端查询能力）。

### D8: 任务附件——`tasks.files/` 目录，抽取共享内容寻址存储助手

任务附件物理存 `<app_data_dir>/tenjee-vault/tasks.files/`（哈希命名，spec §3.1 未定义任务附件目录，此为新增布局约定）。抽取 M2 `notes/attachments.rs` 中通用的内容寻址落盘/引用计数删除逻辑为 `src-tauri/src/blob_store.rs`（纯函数），`notes/attachments.rs` 内部改用它（行为不变），`tasks/attachments.rs` 同样复用。`tasks.db` 0002 迁移建 `attachments`/`taggings` 表（结构同空间库，跨库引用仅存 ID）。

**备选**：附件直接存 tasks.db BLOB 列（否决，大文件撑爆领域库，违背 §3.1「大附件以文件形式存储」原则）；各模块各自复制落盘逻辑（否决，重复代码）。

### D9: 前端架构——根级模块外壳 + 各模块 zustand store，手写视图组件

- **应用外壳**：`src/App.tsx` 改为根路由 + 模块导航（笔记/任务/日程），NotesApp 降为其中一个路由；导航选中态入 URL（`/#/tasks`、`/#/calendar`）可深链。设置项新增 `lunar_overlay_enabled`（默认开）、`festivals_enabled`（默认开）、`solar_terms_enabled`（默认开），沿用 M2 的 meta.db `app_config` 设置存取。
- **任务 UI**：列表视图（层级缩进 + 拖拽排序/跨列表移动）、看板视图（按状态四列，拖拽改状态）、智能视图（今日/本周/逾期，纯查询参数不同）、归档视图；快速添加输入框；多选批量操作。
- **日历 UI**：手写月/周/日网格与议程列表（不引日历组件库，与 M2「不引重型组件库」原则一致）；拖拽用 pointer 事件实现统一拖拽 hook（创建框选、移动、调时长、任务拖入日历共用）；农历叠加在日期格内渲染。
- **日期处理**：手写日期工具（周起始默认周一，中国用户习惯；可配置留 M4 设置体系），不引 date-fns/dayjs。

**备选**：引入 FullCalendar/react-big-calendar（否决，体积大、定制农历叠加与拖拽语义反而更困难，违背项目选型原则）；dnd 库（否决，M2 导航树拖拽已手写，沿用同一套）。

### D10: 跨模块联动——懒解析引用 + 降级显示

`events.linked_page_ref`（`space_id:page_id`）与 `linked_task_id` 仅存 ID，展示时懒解析：查询目标存在性，不存在则渲染失效样式（spec：降级显示）。跳转：笔记页面 → 切到笔记模块路由打开该页；任务 → 切到任务模块并选中。任务拖入日历：前端拖拽落点（日期/时间块）→ 调 `create_event` 带 `linked_task_id`，标题默认取任务标题。删除任务/页面不反向清理事件引用（跨库无约束，展示层兜底）。

**备选**：删除时反向清理引用（否决，跨库分布式事务违背 §3.1「主操作 + 补偿」原则，且懒解析已覆盖失效场景）。

## Risks / Trade-offs

- [农历表/节气表数据错误 → 显示错误历法信息，属正确性硬伤] → 表从权威历法数据生成后以多年份对照单测固化（春节/中秋/清明/冬至/闰月案例全覆盖）；表生成脚本随代码入库可复现
- [RRULE 边界（1 月 31 日月重复、闰年 2/29、UNTIL 含当天与否）→ 展开错误] → 专项单测覆盖边界；规则含不支持部件时明确报错，不静默降级
- [提醒补发风暴：长期未开机后启动轰炸通知] → 补发上限截断 + 汇总通知（D6）；持久化已发记录防重启重发
- [macOS/Windows 通知权限被拒 → 提醒完全失效] → 启动时请求权限；权限缺失时降级为应用内横幅并在设置中提示
- [手写日历视图与拖拽交互复杂度高 → 工期与质量风险] → 拖拽 hook 统一抽象、各视图复用；视图渲染与交互逻辑分离，网格计算纯函数可单测；以手动验证清单兜底
- [月视图每格农历换算 + 重复事件展开 → 渲染性能] → 换算 memoize 缓存；展开仅针对可见范围；60fps 目标下这些计算均为微秒级
- [抽取 `blob_store.rs` 触碰 M2 附件代码 → 回归风险] → 纯内部重构，M2 既有附件单测必须全部保持通过

## Migration Plan

1. `tasks.db`：`0002_fts_archive.sql`（`archived_at` 列、`attachments`/`taggings` 表、`tasks_fts` + 触发器 + 存量回填）；`calendar.db`：`0002_reminders_fts.sql`（`event_reminders` 表 + 旧列数据迁入 + `DROP COLUMN reminder_minutes`、`events_fts` + 触发器 + 存量回填）；`meta.db`：`0003_reminder_fires.sql`（`reminder_fires` 表）。全部由既有迁移体系在启动时自动按序应用。
2. 前端依赖新增 `tauri-plugin-notification`（Rust + JS 两侧）。
3. 回滚：pre-1.0 阶段不提供降级迁移；数据文件损坏时走 data-storage 既有的单库隔离/重建能力。

## Open Questions

无——节气/农历表的数据来源（权威历法数据生成脚本）与周起始日（默认周一）已在 D4/D9 决策；其余均为实现期细节。
