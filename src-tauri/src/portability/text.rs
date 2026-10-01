//! Plain-text import and the shared per-item outcome model used by all portable operations.

use serde::{Deserialize, Serialize};

use super::document::{Block, Inline, PortableDocument};
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Success,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemResult<T> {
    pub item: String,
    pub status: ItemStatus,
    pub value: Option<T>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchResult<T> {
    pub items: Vec<ItemResult<T>>,
}

impl<T> Default for BatchResult<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> BatchResult<T> {
    pub fn success(&mut self, item: impl Into<String>, value: T) {
        self.items.push(ItemResult {
            item: item.into(),
            status: ItemStatus::Success,
            value: Some(value),
            reason: None,
        });
    }
    pub fn skipped(&mut self, item: impl Into<String>, reason: impl Into<String>) {
        self.items.push(ItemResult {
            item: item.into(),
            status: ItemStatus::Skipped,
            value: None,
            reason: Some(reason.into()),
        });
    }
    pub fn failed(&mut self, item: impl Into<String>, reason: impl Into<String>) {
        self.items.push(ItemResult {
            item: item.into(),
            status: ItemStatus::Failed,
            value: None,
            reason: Some(reason.into()),
        });
    }
    pub fn succeeded(&self) -> usize {
        self.items
            .iter()
            .filter(|result| result.status == ItemStatus::Success)
            .count()
    }
    pub fn skipped_count(&self) -> usize {
        self.items
            .iter()
            .filter(|result| result.status == ItemStatus::Skipped)
            .count()
    }
    pub fn failed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|result| result.status == ItemStatus::Failed)
            .count()
    }
}

/// Converts UTF-8 plain text into paragraphs. CRLF is normalized and blank-line runs split
/// paragraphs, retaining single newlines as editable hard breaks.
pub fn parse(bytes: &[u8]) -> VaultResult<PortableDocument> {
    let input = std::str::from_utf8(bytes)
        .map_err(|_| VaultError::Validation("纯文本必须是 UTF-8 编码".into()))?;
    let normalized = input
        .strip_prefix('\u{feff}')
        .unwrap_or(input)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let blocks = normalized
        .split("\n\n")
        .filter_map(|paragraph| {
            let mut content = Vec::new();
            for (index, line) in paragraph.lines().enumerate() {
                if index != 0 {
                    content.push(Inline::HardBreak);
                }
                if !line.is_empty() {
                    content.push(Inline::Text {
                        text: line.to_owned(),
                        marks: vec![],
                    });
                }
            }
            (!content.is_empty()).then_some(Block::Paragraph { content })
        })
        .collect();
    Ok(PortableDocument { blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_mixed_results_without_fabricating_successes() {
        let mut results = BatchResult::default();
        results.success("good.txt", "page-1");
        results.skipped("duplicate.txt", "用户取消覆盖");
        results.failed("bad.txt", "不是 UTF-8");
        assert_eq!(
            (
                results.succeeded(),
                results.skipped_count(),
                results.failed_count()
            ),
            (1, 1, 1)
        );
        assert_eq!(results.items[2].reason.as_deref(), Some("不是 UTF-8"));
        assert!(parse(b"\xff").is_err());
    }

    #[test]
    fn text_becomes_editable_paragraphs_and_breaks() {
        let document = parse("第一行\r\n第二行\r\n\r\n第三段".as_bytes()).unwrap();
        assert_eq!(document.blocks.len(), 2);
        assert!(matches!(document.blocks[0], Block::Paragraph { .. }));
    }
}
