# Tenjee Vault — 产品与技术规格说明书

版本：v0.3（草案）
日期：2026-09-20
状态：待评审

---

## 1. 概述

Tenjee Vault 是一款**纯本地离线**的个人生产力工具，集成三大核心模块：

1. 日程管理（Calendar）
2. 任务管理（Tasks）
3. 笔记（对标 Microsoft OneNote 全部离线功能，含**加密分区**）

敏感信息不设独立密码管理模块，而是通过**笔记加密分区**承载（参考 Evernote/OneNote 的分区保护）：用户为某个分区设定独立密码，分区内容整体加密——密钥、账号等秘密可与上下文笔记存放在一起。

所有数据存储于本地 SQLite 数据库，加密分区的密钥由分区密码实时派生、永不落盘，无任何云端依赖。

### 1.1 设计目标

- **离线优先**：所有功能在无网络环境下 100% 可用
- **数据主权**：数据完全属于用户，存储于本地文件，可随时备份/迁移
- **安全可信**：加密分区采用行业标准加密（AES-256-GCM + Argon2id），密钥不落盘
- **统一体验**：各模块共享搜索、标签、附件、快捷键体系

### 1.2 非目标（Out of Scope）

- 云同步 / 多设备同步（v1 不支持，仅提供手动导入导出）
- 多人协作、分享
- 移动端（v1 仅桌面端；Tauri 架构为未来移动端预留可能）
- 独立密码管理器（由加密分区替代，见 §4.3）

---

## 2. 技术栈

| 层 | 选型 | 说明 |
|---|---|---|
| 应用框架 | Tauri 2.x | 跨平台桌面（Windows / macOS / Linux），小体积、低内存 |
| 前端 | React 19 + TypeScript | 运行于 Tauri WebView |
| 后端逻辑 | Rust（Tauri commands） | 数据库访问、加密、文件操作 |
| 数据库 | SQLite（via `rusqlite` 或 `sqlx`） | **多库分离架构**，见 §3 |
| 加密 | 应用层字段加密（AES-256-GCM + Argon2id） | 密钥由分区密码实时派生，不落盘，见 §6 |
| 全文搜索 | SQLite FTS5（各库独立索引，应用层聚合） | 见 §3.3 |
| 富文本编辑 | TipTap | 笔记编辑器，流式文档，JSON 存储 |
| 加密库 | RustCrypto（aes-gcm, argon2） | 加密分区 |
| 打包分发 | Tauri Bundler（Windows 便携 exe / macOS dmg / Linux AppImage，各平台仅构建本机目标） | 免安装可执行文件 |

### 2.1 项目结构（建议）

```
tenjee-vault/
├── src/                  # 前端 (React/TS)
│   ├── modules/
│   │   ├── calendar/     # 日程
│   │   ├── tasks/        # 任务
│   │   └── notes/        # 笔记（含加密分区）
│   └── shared/           # 搜索、标签、UI 组件
├── src-tauri/            # Rust 后端
│   ├── src/
│   │   ├── db/           # SQLite 迁移与访问层
│   │   ├── crypto/       # 加密
│   │   ├── search/       # FTS5
│   │   └── commands/     # Tauri commands
│   └── migrations/       # SQL 迁移文件
└── docs/
```

---

## 3. 数据存储设计

### 3.1 多库分离架构

为降低单库损坏导致全部数据丢失的风险，采用**主库 + 领域分库**模式：

```
<app_data_dir>/tenjee-vault/
├── meta.db                 # 主库：配置、分库注册表、统一标签
├── tasks.db                # 任务
├── calendar.db             # 日程
├── spaces/
│   ├── <space_id>.db       # 笔记空间库：每个「空间」一个文件（含加密分区）
│   └── <space_id>.files/   # 该空间的附件（哈希命名；加密分区的附件亦加密）
└── backups/                # 可选自动备份目录
```

**分离规则**
- `meta.db`（主库）：应用配置、用户偏好、`spaces` 注册表、标签字典。体积小、写入少、损坏概率最低；即使损坏也可重建（配置重置，数据不丢）
- 领域库各一：`tasks.db` / `calendar.db`
- 笔记按**空间（Space）**分库：用户可建多个空间（默认一个），每个空间 = 独立 `.db` + 独立附件目录。空间是笔记本的上层容器。大附件以文件形式存储，不占库体积
- 任一库损坏仅影响该领域/空间；支持单库备份、修复（`PRAGMA integrity_check` + `.recover`）、替换

**跨库约束**
- 跨库引用（如事件关联笔记页、任务关联标签）仅存 ID，不做外键约束，应用层容错（目标不存在时降级显示）
- 跨库操作不使用分布式事务；以「主操作 + 补偿」方式保证最终一致（如删除空间时清理 meta.db 注册项）
- 启动时对每个库执行 `PRAGMA integrity_check` 快检，异常库隔离并提示修复

### 3.2 核心数据模型（ER 概要）

```sql
-- meta.db
app_config(key, value)                                     -- 偏好设置
spaces(id, name, db_file, sort_order, created_at)
tags(id, name, color)

-- spaces/<space_id>.db
notebooks(id, name, color, sort_order, created_at, updated_at)
section_groups(id, notebook_id, parent_group_id NULL, name, sort_order)
sections(id, notebook_id, section_group_id NULL, name, color, sort_order,
         is_encrypted, kdf_salt NULL, kdf_params NULL,
         verifier NULL, wrapped_dsk NULL)                 -- 加密分区：KDF 参数、验证器、被包裹的数据密钥
pages(id, section_id, parent_page_id NULL, title, content, -- TipTap JSON（加密分区则为密文）
      created_at, updated_at, sort_order, is_deleted)
page_versions(id, page_id, content, created_at)            -- 加密分区中同样为密文
attachments(id, entity_type, entity_id, file_name, mime, size, hash, created_at)
taggings(tag_id, entity_type, entity_id)

-- tasks.db
task_lists(id, name, color, sort_order)
tasks(id, list_id, title, notes, status,                  -- todo/in_progress/done/cancelled
      priority, due_date, due_time, reminder_at, recurrence_rule,
      parent_task_id NULL, sort_order, completed_at, created_at, updated_at)
attachments(...) / taggings(...)

-- calendar.db
events(id, title, description, location,
       start_at, end_at, all_day, timezone,
       recurrence_rule NULL,                              -- RFC 5545 RRULE（公历）
       lunar_recurrence NULL,                             -- 农历重复规则 JSON: {month,day,leap_month}
       reminder_minutes, color, linked_page_ref NULL, linked_task_id NULL,
       created_at, updated_at)                            -- linked_page_ref = "space_id:page_id"
event_exceptions(id, event_id, original_start_at, new_start_at, is_cancelled)
```

### 3.3 搜索索引

- 每个库内建独立 FTS5 虚拟表：
  - 空间库：页面标题 + 正文
  - `tasks.db`：任务标题 + 备注；`calendar.db`：事件标题 + 描述
  - **加密分区**：锁定时完全不参与索引（标题与正文均不可见）；解锁后将会话内解密内容写入内存临时 FTS 表，锁定即销毁，不落盘
- 全局搜索：并行查询各库索引，应用层聚合排序
- 编辑器内容变更时增量更新索引

---

## 4. 功能规格

### 4.1 日程管理（Calendar）

**视图**
- 月视图 / 周视图 / 日视图 / 议程（Agenda）列表视图
- 拖拽创建与调整事件（拖时间块、改日期）

**事件**
- 字段：标题、描述、地点、开始/结束时间、全天、时区、颜色、提醒
- 重复规则：支持 RFC 5545 RRULE（每日/每周/每月/每年、自定义间隔、结束条件），支持单次例外修改/取消
- 提醒：应用内通知（Tauri Notification API），可设多个提前量
- 联动：事件可关联笔记页面与任务

**农历支持（Chinese Lunar Calendar）**
- 视图叠加：月/周/日视图在公历日期旁显示对应农历日期（初一/十五、节气），可在设置中开关
- 农历事件：事件可按农历创建与重复（如"农历生日"、"每年正月初一"），支持闰月规则（默认忽略闰月 / 指定闰月）
- 内置节日：传统节日（春节、清明、端午、中秋等）与二十四节气显示，可开关
- 实现：纯本地历法换算（Rust 端实现或内置查表 1900–2100 年），不依赖网络；.ics 导出时农历重复事件展开为公历 RRULE 或固定日期序列

**导入导出**
- 导入/导出 iCalendar（.ics）格式

### 4.2 任务管理（Tasks）

**列表与层级**
- 自定义任务列表（如：收件箱、工作、个人）
- 子任务（多级）
- 看板视图（按状态分列）+ 列表视图 + 今日/本周/逾期智能视图

**任务属性**
- 状态：待办 / 进行中 / 完成 / 取消
- 优先级：无 / 低 / 中 / 高
- 截止日期与时间、提醒、重复规则
- 备注（富文本）、标签、附件
- 完成时间记录，已完成任务归档与恢复

**操作**
- 拖拽排序、批量操作、快速添加（自然语言解析可选，如"明天下午3点 开会"）
- 与日程联动：拖到日历可生成事件

### 4.3 笔记（对标 OneNote 离线功能全集）

**组织结构**
- 空间（Space）→ 笔记本（Notebook）→ 分区组（Section Group，可嵌套）→ 分区（Section）→ 页面（Page）→ 子页面（多级缩进）
- 空间为独立数据库文件（见 §3.1），支持新建、重命名、归档、单独备份
- 全部支持重命名、拖拽移动、排序、颜色标记
- 分区可加密（见下文「加密分区」）

**加密分区（敏感信息保护，替代独立密码管理器）**
- 任意分区可设置独立密码进行加密（参考 Evernote/OneNote 分区保护）；秘密可与上下文笔记同页存放（如服务器账号 + 部署笔记）
- 密钥由分区密码经 Argon2id 实时派生，**永不落盘**；`sections` 表仅存 KDF salt/参数与验证器
- 加密范围：分区下所有页面正文、历史版本、附件（文件内容加密存储）
- 锁定时：分区内容不可见、不参与搜索与全局索引、页面标题可显示（可选设置：标题也隐藏）
- 解锁：输入密码解锁当前分区，会话内有效；闲置 N 分钟（默认 5）/ 手动锁定 / 退出应用后自动锁定并清零内存密钥
- 修改/移除密码：验证旧密码后重加密分区密钥材料（页面数据无需重加密——采用「分区数据密钥 + 密码包裹」两层结构，见 §6）
- 忘记分区密码 = 该分区数据不可恢复（无后门），设置时强制确认提示
- 内置密码生成器小工具（用于生成高强度秘密写入笔记）

**编辑器（MVP 为流式文档，TipTap）**
- 富文本：标题层级、粗斜体、下划线、删除线、高亮、字体/字号/颜色
- 列表：项目符号、编号、待办复选框（可与任务模块打通）
- 表格：插入/编辑表格
- 图片：粘贴、拖入、调整大小
- 文件附件：任意文件嵌入页面（存所属空间的 `.files/` 目录）
- 手写/绘图：绘图模式，笔画以 **SVG** 存储并嵌入页面
- 录音：音频录制并嵌入页面（可选，v1.1）
- 链接：页面间双向链接 `[[页面名]]`、外部 URL
- 页面模板：内置模板 + 自定义模板
- 标签：OneNote 风格内置标记（待办、重要、问题、星标…）+ 自定义标签，可跨页面汇总查询
- （v1.1+）OneNote 式自由画布：文本框任意位置放置，MVP 不包含

**页面管理**
- 版本历史：自动快照，可查看/回滚
- 回收站：删除页面进入回收站，可恢复/彻底删除
- 最近使用页面列表

**搜索与导航**
- 全文搜索（FTS5）：按笔记本/分区过滤，关键词高亮
- 快速跳转（Ctrl+P 类命令面板）
- 标签汇总视图

**导入导出**
- 导出：单页/分区 → Markdown、HTML、PDF（加密分区需先解锁）
- 导入：Markdown、HTML、纯文本；（可选）OneNote .one 导入为远期目标
- 打印

---

## 5. 通用功能

- **命令面板**：全局搜索 + 快捷操作入口
- **统一标签体系**：跨模块标签与筛选
- **全局快捷键**：可自定义；默认含快速笔记、快速任务、全局搜索、锁定全部加密分区
- **备份**：手动导出备份文件（加密分区保持密文备份）；可选定时自动备份到指定目录（保留最近 N 份）
- **多语言**：v1 中文 + 英文
- **主题**：亮色 / 暗色 / 跟随系统
- **系统托盘**：最小化到托盘、快速入口

---

## 6. 安全设计

**核心原则：密钥永不落盘。** 每个加密分区的密码是唯一秘密；应用不存储任何加密密钥、明文密码或恢复令牌。

| 项 | 方案 |
|---|---|
| 两层密钥结构 | 每个加密分区生成随机 **分区数据密钥（DSK）**；页面/版本/附件用 DSK 加密。分区密码经 Argon2id 派生 **密钥加密密钥（KEK）** 包裹 DSK——修改密码只需重包裹 DSK，无需重加密数据 |
| 密钥派生 | 分区密码 + 随机 salt → Argon2id（m=64MB, t=3, p=4）→ KEK，仅驻留内存（`zeroize` 保护），锁定/退出即清零 |
| 验证机制 | `sections.verifier`：KEK 加密固定已知明文，解锁时解密比对验证密码正确性 |
| 对称加密 | AES-256-GCM，每页/每版本/每附件随机 nonce |
| 密钥存储 | **无**。不落盘、不写密钥文件、不使用系统钥匙串；DB 中仅存 wrapped DSK、salt、KDF 参数 |
| 加密分区搜索 | 解锁会话内的解密索引仅存内存临时 FTS 表，锁定即销毁（见 §3.3） |
| 锁定 | 闲置（默认 5 分钟）/ 手动 / 退出触发；锁定后 KEK/DSK 内存清零，内存索引销毁 |
| 剪贴板 | 从加密分区复制的内容 30 秒后自动清空（可选设置） |
| 备份文件 | 加密分区的密文原样备份（含 wrapped DSK 与 KDF 参数），恢复后仍需原分区密码解锁 |
| 数据恢复 | 无后门：忘记分区密码即该分区不可恢复，设置密码时强制确认提示 |

---

## 7. 非功能性需求

- **性能**：冷启动 < 2s；10 万条笔记页面下全文搜索 < 300ms；UI 操作 60fps
- **可靠性**：数据库 WAL 模式；写入事务化；异常退出不丢数据
- **可测试性**：Rust 核心逻辑单元测试；加密模块 100% 覆盖
- **可访问性**：键盘全操作、合理对比度

---

## 8. 里程碑（建议）

| 阶段 | 内容 |
|---|---|
| M1 | 项目骨架、多库架构与迁移体系、加密原语（Argon2id/AES-GCM/密钥包裹） |
| M2 | 笔记模块（空间/组织结构 + TipTap 编辑器 + SVG 绘图 + 加密分区 + 搜索） |
| M3 | 任务模块 + 日程模块（含农历） |
| M4 | 导入导出、备份、快捷键、托盘、打磨与发布 v1.0 |
| v1.1+ | 自由画布、录音、可选同步（WebDAV） |

---

## 9. 已决事项

| 决策项 | 结论 |
|---|---|
| 前端框架 | React 19 |
| 富文本编辑器 | TipTap（MVP 流式文档，自由画布延后至 v1.1+） |
| 绘图存储 | SVG |
| 敏感信息保护 | 不设独立密码管理器；笔记加密分区（分区密码派生密钥，永不落盘） |
| 数据库架构 | 主库 meta.db + 领域分库 + 笔记按空间分库（§3.1） |
| 农历 | 日历支持农历显示、农历重复事件、传统节日与节气 |
