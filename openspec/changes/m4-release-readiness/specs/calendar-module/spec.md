# Calendar Module Spec Delta

## ADDED Requirements

### Requirement: iCalendar 互操作入口
日程模块 SHALL 提供 `.ics` 文件导入和按日期范围、单个事件或全部事件导出的入口，遵守 data-portability 的 UID 冲突、格式兼容、逐项报告和安全文件写入规则。

#### Scenario: 导出选定日期范围
- **WHEN** 用户选择开始与结束日期导出日历
- **THEN** 导出文件仅包含与范围相交的非重复事件及重复系列实例，并能被常见 iCalendar 客户端解析

### Requirement: 农历事件的 iCalendar 展开边界
农历重复规则 SHALL NOT 被错误表示为公历 RRULE。导出农历重复事件时系统 SHALL 要求有限范围并在该范围内生成带共同系列标识的公历实例；导入由本应用生成的这些实例 SHALL 可选择恢复为独立公历事件，且 SHALL NOT 猜测重建农历规则。

#### Scenario: 未指定农历导出范围
- **WHEN** 用户尝试导出无限农历重复事件但未指定结束日期
- **THEN** 系统要求选择有限结束日期且不生成不完整文件

