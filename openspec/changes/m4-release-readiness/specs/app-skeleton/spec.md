# App Skeleton Spec Delta

## ADDED Requirements

### Requirement: 稳定应用身份
桌面应用 SHALL 使用稳定产品名“Tenjee Vault”、稳定 bundle identifier 与语义化版本，并在 Windows、macOS 和 Linux 上使用一致品牌图标。升级版本 SHALL 保持应用标识与数据目录解析不变。

#### Scenario: 升级安装识别同一应用
- **WHEN** 用户在已安装旧版本上安装 v1.0
- **THEN** 操作系统识别为同一应用升级，应用继续读取原数据目录

### Requirement: 平台安装包
项目 SHALL 能在对应原生构建环境产出 Windows 安装包、macOS 应用包/磁盘映像与 Linux AppImage/deb 产物；安装包 SHALL 包含运行所需前端静态资源、Tauri 权限与平台图标，且启动不依赖开发服务器。

#### Scenario: 全新安装启动
- **WHEN** 用户在受支持平台安装并首次启动发布包
- **THEN** 应用打开主窗口、初始化本地数据布局，并可离线使用三个核心模块

### Requirement: 托盘参与应用生命周期
应用骨架 SHALL 支持主窗口隐藏后由托盘重新显示，并区分“关闭窗口”与“退出应用”；最后一个窗口隐藏时后台提醒与自动备份服务按设置继续运行，明确退出时执行安全关闭钩子。

#### Scenario: 隐藏后恢复窗口
- **WHEN** 主窗口已隐藏且用户激活托盘图标
- **THEN** 原主窗口恢复、获得焦点且保持隐藏前的模块与导航状态

