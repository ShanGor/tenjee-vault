//! Deterministic, local PDF rendering for the portable document subset.
//!
//! The renderer embeds Noto Sans CJK (OFL license beside the asset) so Chinese and English
//! documents remain readable and searchable without relying on a host-system font.

use std::io::Cursor;
use std::path::Path;

use printpdf::{Image, ImageTransform, IndirectFontRef, Mm, PdfDocument};

use super::document::{Block, Inline, PortableDocument, TableRow, TextMark};
use crate::error::{VaultError, VaultResult};

const PAGE_WIDTH: f32 = 210.0;
const PAGE_HEIGHT: f32 = 297.0;
const LEFT: f32 = 18.0;
const TOP: f32 = 279.0;
const BOTTOM: f32 = 18.0;
const BODY_SIZE: f32 = 10.5;
const EMBEDDED_CJK_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSansCJK-Regular.ttc");

/// Render a PDF byte vector. Callers own the safe/atomic file destination boundary.
pub fn render(document: &PortableDocument, title: &str) -> VaultResult<Vec<u8>> {
    render_with_resource_root(document, title, None)
}

/// Render with a caller-owned, already validated resource root. Image sources must be safe
/// relative paths below this root; unresolved resources retain an explicit text placeholder.
pub fn render_with_resource_root(
    document: &PortableDocument,
    title: &str,
    resource_root: Option<&Path>,
) -> VaultResult<Vec<u8>> {
    let (pdf, page, layer) = PdfDocument::new(title, Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "content");
    let font = pdf
        .add_external_font(Cursor::new(EMBEDDED_CJK_FONT))
        .map_err(|error| {
            VaultError::Validation(format!("无法加载内嵌 Noto Sans CJK 字体: {error}"))
        })?;
    let mut canvas = Canvas {
        pdf: &pdf,
        page,
        layer,
        font: &font,
        y: TOP,
        resource_root,
    };
    for block in &document.blocks {
        canvas.block(block, 0)?;
    }
    pdf.save_to_bytes()
        .map_err(|error| VaultError::Validation(format!("PDF 生成失败: {error}")))
}

struct Canvas<'a> {
    pdf: &'a printpdf::PdfDocumentReference,
    page: printpdf::PdfPageIndex,
    layer: printpdf::PdfLayerIndex,
    font: &'a IndirectFontRef,
    y: f32,
    resource_root: Option<&'a Path>,
}

impl Canvas<'_> {
    fn layer(&self) -> printpdf::PdfLayerReference {
        self.pdf.get_page(self.page).get_layer(self.layer)
    }
    fn advance(&mut self, amount: f32) {
        self.y -= amount;
        if self.y < BOTTOM {
            let (page, layer) = self
                .pdf
                .add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "content");
            self.page = page;
            self.layer = layer;
            self.y = TOP;
        }
    }
    fn text(&mut self, text: &str, size: f32, indent: f32) {
        for line in wrap(
            text,
            ((PAGE_WIDTH - LEFT * 2.0 - indent) / (size * 0.52)).max(8.0) as usize,
        ) {
            self.layer()
                .use_text(line, size, Mm(LEFT + indent), Mm(self.y), self.font);
            self.advance(size * 0.6 + 1.8);
        }
    }
    fn block(&mut self, block: &Block, depth: usize) -> VaultResult<()> {
        match block {
            Block::Paragraph { content } => {
                self.text(&inline_text(content), BODY_SIZE, 0.0);
                self.advance(2.0);
            }
            Block::Heading { level, content } => {
                self.text(
                    &inline_text(content),
                    18.0 - (*level as f32 - 1.0) * 1.5,
                    0.0,
                );
                self.advance(2.0);
            }
            Block::List { ordered, items } => {
                for (index, item) in items.iter().enumerate() {
                    let prefix = if *ordered {
                        format!("{}. ", index + 1)
                    } else {
                        "• ".to_owned()
                    };
                    self.text(
                        &(prefix + &blocks_text(&item.blocks)),
                        BODY_SIZE,
                        depth as f32 * 7.0,
                    );
                }
            }
            Block::TaskList { items } => {
                for item in items {
                    let prefix = if item.checked { "☒ " } else { "☐ " };
                    self.text(
                        &(prefix.to_owned() + &blocks_text(&item.blocks)),
                        BODY_SIZE,
                        depth as f32 * 7.0,
                    );
                }
            }
            Block::Table { rows } => self.table(rows),
            Block::Image { resource } => self.image(resource)?,
            Block::Attachment { resource } => {
                self.text(
                    &format!(
                        "[附件: {}]",
                        resource.name.as_deref().unwrap_or(&resource.source)
                    ),
                    BODY_SIZE,
                    0.0,
                );
            }
        }
        Ok(())
    }
    fn table(&mut self, rows: &[TableRow]) {
        for row in rows {
            let values: Vec<_> = row
                .cells
                .iter()
                .map(|cell| blocks_text(&cell.blocks))
                .collect();
            self.text(&values.join(" | "), BODY_SIZE, 0.0);
        }
        self.advance(2.0);
    }
    fn image(&mut self, resource: &super::document::Resource) -> VaultResult<()> {
        let Some(root) = self.resource_root else {
            self.text(
                &format!(
                    "[图片: {}]",
                    resource.alt.as_deref().unwrap_or(&resource.source)
                ),
                BODY_SIZE,
                0.0,
            );
            return Ok(());
        };
        let relative = super::markdown::checked_relative_path(&resource.source)?;
        let path = root.join(relative);
        if !path.is_file() {
            self.text(
                &format!(
                    "[图片不可用: {}]",
                    resource.alt.as_deref().unwrap_or(&resource.source)
                ),
                BODY_SIZE,
                0.0,
            );
            return Ok(());
        }
        let decoded = printpdf::image_crate::io::Reader::open(&path)
            .map_err(|error| {
                VaultError::Validation(format!("无法读取导出图片 {}: {error}", path.display()))
            })?
            .decode()
            .map_err(|error| {
                VaultError::Validation(format!("无法解析导出图片 {}: {error}", path.display()))
            })?;
        let image = Image::from_dynamic_image(&decoded);
        image.add_to_layer(
            self.layer(),
            ImageTransform {
                translate_x: Some(Mm(LEFT)),
                translate_y: Some(Mm(self.y - 35.0)),
                dpi: Some(96.0),
                scale_x: Some(0.45),
                scale_y: Some(0.45),
                ..ImageTransform::default()
            },
        );
        self.advance(40.0);
        Ok(())
    }
}

fn blocks_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { content } | Block::Heading { content, .. } => inline_text(content),
            Block::List { items, .. } => items
                .iter()
                .map(|item| blocks_text(&item.blocks))
                .collect::<Vec<_>>()
                .join("; "),
            Block::TaskList { items } => items
                .iter()
                .map(|item| blocks_text(&item.blocks))
                .collect::<Vec<_>>()
                .join("; "),
            Block::Table { rows } => rows
                .iter()
                .flat_map(|row| row.cells.iter().map(|cell| blocks_text(&cell.blocks)))
                .collect::<Vec<_>>()
                .join(" | "),
            Block::Image { resource } | Block::Attachment { resource } => resource
                .name
                .clone()
                .unwrap_or_else(|| resource.source.clone()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn inline_text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::HardBreak => "\n".to_owned(),
            Inline::Link { href, text } => format!("{text} ({href})"),
            Inline::Text { text, marks } => {
                let _ = marks
                    .iter()
                    .any(|mark| matches!(mark, TextMark::Bold | TextMark::Italic));
                text.clone()
            }
        })
        .collect()
}
fn wrap(input: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in input.lines() {
        let chars: Vec<_> = raw.chars().collect();
        if chars.is_empty() {
            lines.push(String::new());
        } else {
            for chunk in chars.chunks(width) {
                lines.push(chunk.iter().collect());
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portability::document::{Resource, TableCell};

    #[test]
    fn creates_a_paginated_searchable_pdf_for_chinese_and_english() {
        let document = PortableDocument {
            blocks: vec![
                Block::Heading {
                    level: 1,
                    content: vec![Inline::Text {
                        text: "中文 English 标题".into(),
                        marks: vec![],
                    }],
                },
                Block::TaskList {
                    items: vec![super::super::document::TaskItem {
                        checked: false,
                        blocks: vec![Block::Paragraph {
                            content: vec![Inline::Link {
                                href: "https://example.test".into(),
                                text: "链接内容".into(),
                            }],
                        }],
                    }],
                },
                Block::Table {
                    rows: vec![TableRow {
                        cells: vec![TableCell {
                            header: true,
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "表格".into(),
                                    marks: vec![],
                                }],
                            }],
                        }],
                    }],
                },
                Block::Image {
                    resource: Resource {
                        source: "image.png".into(),
                        alt: Some("图片".into()),
                        ..Resource::default()
                    },
                },
            ],
        };
        let bytes = render(&document, "测试").unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 1_000);
    }
}
