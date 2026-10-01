//! 跨格式数据可移植领域层。
//!
//! 所有外部格式先转换为受限的 `PortableDocument`，再交给对应 renderer；
//! 这样 TipTap、Markdown、HTML 与 PDF 不需要两两转换。

pub mod document;
pub mod export;
pub mod file_boundary;
pub mod html;
pub mod ical;
pub mod markdown;
pub mod notes;
pub mod pdf;
pub mod text;
