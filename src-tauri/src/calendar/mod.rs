//! 日程领域层（M3）：事件管理、重复规则、农历换算、传统节日与节气。
//! 分层见 design D1/D2/D3/D4。

pub mod events;
pub mod festivals;
pub mod lunar;
pub mod lunar_data;
pub mod lunar_rule;
pub mod rrule;
