# App Skeleton Specification

## Purpose

定义 Tenjee Vault 桌面应用的初始骨架：应用必须能够作为 Tauri 2.x 桌面应用在 Windows、macOS、Linux 上构建与启动，前后端分工明确，目录结构为后续模块（日程、任务、笔记）的扩展预留清晰位置。

## Requirements

### Requirement: 跨平台桌面应用骨架
系统 SHALL 提供一个基于 Tauri 2.x 的桌面应用骨架，能够在 Windows、macOS、Linux 三个桌面平台上构建并启动。前端 SHALL 运行于 Tauri WebView，使用 React 与 TypeScript；业务后端逻辑（数据库访问、加密、文件操作）SHALL 位于 Rust 侧并通过 Tauri commands 暴露给前端。

#### Scenario: 应用启动
- **WHEN** 用户在任一受支持的桌面平台上启动应用
- **THEN** 应用窗口正常打开，前端渲染出最小可验证界面，前端能够通过 Tauri command 调用 Rust 后端并获得响应

#### Scenario: 跨平台构建
- **WHEN** 在 Windows、macOS、Linux 各自的构建环境中执行构建
- **THEN** 构建均成功产出对应平台的可安装包或可执行文件

### Requirement: 项目目录结构
代码仓库 SHALL 采用如下顶层组织：`src/` 存放 React/TypeScript 前端（含 `modules/` 下的 calendar、tasks、notes 模块目录与 `shared/` 共享目录），`src-tauri/` 存放 Rust 后端（含 `db/`、`crypto/`、`search/`、`commands/` 子目录与 `migrations/` 迁移文件目录）。

#### Scenario: 新增模块接入骨架
- **WHEN** 开发者向 `src/modules/` 下添加一个新模块目录
- **THEN** 无需改动骨架层代码即可将该模块挂入前端构建与路由（如适用）

### Requirement: 启动性能基线
在常规桌面硬件上，应用冷启动时间 SHALL 小于 2 秒（不含首次运行时的数据库初始化）。

#### Scenario: 冷启动计时
- **WHEN** 对已初始化数据目录的应用执行冷启动
- **THEN** 从进程启动到前端界面可交互的时间小于 2 秒
