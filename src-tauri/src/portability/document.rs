//! 用于导入、导出和 PDF 渲染的受限文档模型，以及与 TipTap JSON 的双向转换。
//!
//! 该模型有意只表示产品支持的节点。遇到未知 TipTap 节点时返回校验错误，
//! 以免导出时静默丢失用户内容。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PortableDocument {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph {
        content: Vec<Inline>,
    },
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Blockquote {
        blocks: Vec<Block>,
    },
    Details {
        open: bool,
        summary: Vec<Inline>,
        blocks: Vec<Block>,
    },
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    HorizontalRule,
    List {
        ordered: bool,
        items: Vec<ListItem>,
    },
    TaskList {
        items: Vec<TaskItem>,
    },
    Table {
        rows: Vec<TableRow>,
    },
    Image {
        resource: Resource,
    },
    Attachment {
        resource: Resource,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TaskItem {
    pub checked: bool,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub header: bool,
    pub blocks: Vec<Block>,
    #[serde(default = "one")]
    pub colspan: u64,
    #[serde(default = "one")]
    pub rowspan: u64,
}

fn one() -> u64 {
    1
}
impl Default for TableCell {
    fn default() -> Self {
        Self {
            header: false,
            blocks: Vec::new(),
            colspan: 1,
            rowspan: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text {
        text: String,
        marks: Vec<TextMark>,
    },
    HardBreak,
    /// PageLink 的稳定页面 id 以 `page://<id>` 形式表达，普通链接保持原 URL。
    Link {
        href: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextMark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Highlight,
    Link {
        href: String,
    },
    TextStyle {
        color: Option<String>,
        font_size: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Resource {
    /// 图片可以是相对导出路径、文件 URI 或 `attachment://<id>`；附件为稳定附件 id。
    pub source: String,
    pub name: Option<String>,
    pub alt: Option<String>,
    pub title: Option<String>,
    pub width: Option<String>,
    pub size: Option<u64>,
}

impl PortableDocument {
    pub fn from_tiptap_json(value: &Value) -> VaultResult<Self> {
        let document = object(value, "TipTap 文档")?;
        if string_field(document, "type")? != "doc" {
            return Err(VaultError::Validation("TipTap 根节点必须是 doc".into()));
        }
        Ok(Self {
            blocks: parse_blocks(contents(document)?)?,
        })
    }

    pub fn to_tiptap_json(&self) -> Value {
        json!({
            "type": "doc",
            "content": self.blocks.iter().map(block_to_json).collect::<Vec<_>>(),
        })
    }
}

fn parse_blocks(nodes: &[Value]) -> VaultResult<Vec<Block>> {
    nodes.iter().map(parse_block).collect()
}

fn parse_block(value: &Value) -> VaultResult<Block> {
    let node = object(value, "TipTap 节点")?;
    let node_type = string_field(node, "type")?;
    match node_type {
        "paragraph" => Ok(Block::Paragraph {
            content: parse_inlines(contents(node)?)?,
        }),
        "blockquote" => Ok(Block::Blockquote {
            blocks: parse_blocks(contents(node)?)?,
        }),
        "details" => {
            let children = contents(node)?;
            if children.len() != 2 {
                return Err(VaultError::Validation(
                    "details requires a summary and content".into(),
                ));
            }
            let summary = object(&children[0], "details summary")?;
            let body = object(&children[1], "details content")?;
            if string_field(summary, "type")? != "detailsSummary"
                || string_field(body, "type")? != "detailsContent"
                || contents(body)?.is_empty()
            {
                return Err(VaultError::Validation("Invalid details structure".into()));
            }
            Ok(Block::Details {
                open: optional_bool(attrs(node), "open")?.unwrap_or(false),
                summary: parse_inlines(contents(summary)?)?,
                blocks: parse_blocks(contents(body)?)?,
            })
        }
        "codeBlock" => {
            let mut text = String::new();
            for value in contents(node)? {
                let child = object(value, "代码块内容")?;
                if string_field(child, "type")? != "text" {
                    return Err(VaultError::Validation("代码块只能包含文本".into()));
                }
                text.push_str(string_field(child, "text")?);
            }
            Ok(Block::CodeBlock {
                language: optional_string(attrs(node), "language")?,
                text,
            })
        }
        "horizontalRule" => Ok(Block::HorizontalRule),
        "heading" => {
            let level = optional_u64(attrs(node), "level")?.unwrap_or(1);
            if !(1..=6).contains(&level) {
                return Err(VaultError::Validation(format!("标题级别 {level} 非法")));
            }
            Ok(Block::Heading {
                level: level as u8,
                content: parse_inlines(contents(node)?)?,
            })
        }
        "bulletList" => Ok(Block::List {
            ordered: false,
            items: parse_list_items(contents(node)?)?,
        }),
        "orderedList" => Ok(Block::List {
            ordered: true,
            items: parse_list_items(contents(node)?)?,
        }),
        "taskList" => Ok(Block::TaskList {
            items: parse_task_items(contents(node)?)?,
        }),
        "table" => Ok(Block::Table {
            rows: parse_table_rows(contents(node)?)?,
        }),
        "image" => Ok(Block::Image {
            resource: resource_from_attrs(attrs(node), "image")?,
        }),
        "attachmentBlock" => Ok(Block::Attachment {
            resource: attachment_from_attrs(attrs(node))?,
        }),
        other => Err(VaultError::Validation(format!(
            "不支持的 TipTap 块节点 {other}"
        ))),
    }
}

fn parse_list_items(nodes: &[Value]) -> VaultResult<Vec<ListItem>> {
    nodes
        .iter()
        .map(|value| {
            let node = object(value, "列表项")?;
            if string_field(node, "type")? != "listItem" {
                return Err(VaultError::Validation("列表只能包含 listItem".into()));
            }
            Ok(ListItem {
                blocks: parse_blocks(contents(node)?)?,
            })
        })
        .collect()
}

fn parse_task_items(nodes: &[Value]) -> VaultResult<Vec<TaskItem>> {
    nodes
        .iter()
        .map(|value| {
            let node = object(value, "待办项")?;
            if string_field(node, "type")? != "taskItem" {
                return Err(VaultError::Validation("待办列表只能包含 taskItem".into()));
            }
            Ok(TaskItem {
                checked: optional_bool(attrs(node), "checked")?.unwrap_or(false),
                blocks: parse_blocks(contents(node)?)?,
            })
        })
        .collect()
}

fn parse_table_rows(nodes: &[Value]) -> VaultResult<Vec<TableRow>> {
    nodes
        .iter()
        .map(|value| {
            let row = object(value, "表格行")?;
            if string_field(row, "type")? != "tableRow" {
                return Err(VaultError::Validation("表格只能包含 tableRow".into()));
            }
            let cells = contents(row)?
                .iter()
                .map(|value| {
                    let cell = object(value, "表格单元格")?;
                    let kind = string_field(cell, "type")?;
                    let header = match kind {
                        "tableHeader" => true,
                        "tableCell" => false,
                        _ => {
                            return Err(VaultError::Validation(
                                "表格行只能包含 tableHeader 或 tableCell".into(),
                            ))
                        }
                    };
                    Ok(TableCell {
                        header,
                        blocks: parse_blocks(contents(cell)?)?,
                        colspan: optional_u64(attrs(cell), "colspan")?.unwrap_or(1).max(1),
                        rowspan: optional_u64(attrs(cell), "rowspan")?.unwrap_or(1).max(1),
                    })
                })
                .collect::<VaultResult<Vec<_>>>()?;
            Ok(TableRow { cells })
        })
        .collect()
}

fn parse_inlines(nodes: &[Value]) -> VaultResult<Vec<Inline>> {
    nodes
        .iter()
        .map(|value| {
            let node = object(value, "行内节点")?;
            match string_field(node, "type")? {
                "text" => {
                    let text = string_field(node, "text")?.to_string();
                    let marks = parse_marks(node.get("marks"))?;
                    if let [TextMark::Link { href }] = marks.as_slice() {
                        Ok(Inline::Link {
                            href: href.clone(),
                            text,
                        })
                    } else {
                        Ok(Inline::Text { text, marks })
                    }
                }
                "hardBreak" => Ok(Inline::HardBreak),
                "pageLink" => {
                    let attrs = attrs(node);
                    let page_id = required_string(attrs, "pageId", "页面链接")?;
                    let label = optional_string(attrs, "label")?.unwrap_or_default();
                    Ok(Inline::Link {
                        href: format!("page://{page_id}"),
                        text: label,
                    })
                }
                other => Err(VaultError::Validation(format!(
                    "不支持的 TipTap 行内节点 {other}"
                ))),
            }
        })
        .collect()
}

fn parse_marks(value: Option<&Value>) -> VaultResult<Vec<TextMark>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let marks = value
        .as_array()
        .ok_or_else(|| VaultError::Validation("TipTap marks 必须是数组".into()))?;
    marks
        .iter()
        .map(|value| {
            let mark = object(value, "TipTap 标记")?;
            match string_field(mark, "type")? {
                "bold" => Ok(TextMark::Bold),
                "italic" => Ok(TextMark::Italic),
                "underline" => Ok(TextMark::Underline),
                "strike" => Ok(TextMark::Strike),
                "code" => Ok(TextMark::Code),
                "highlight" => Ok(TextMark::Highlight),
                "link" => Ok(TextMark::Link {
                    href: required_string(attrs(mark), "href", "链接")?,
                }),
                "textStyle" => Ok(TextMark::TextStyle {
                    color: optional_string(attrs(mark), "color")?,
                    font_size: optional_string(attrs(mark), "fontSize")?,
                }),
                other => Err(VaultError::Validation(format!(
                    "不支持的 TipTap 文本标记 {other}"
                ))),
            }
        })
        .collect()
}

fn resource_from_attrs(attrs: &Map<String, Value>, kind: &str) -> VaultResult<Resource> {
    Ok(Resource {
        source: required_string(attrs, "src", kind)?,
        name: None,
        alt: optional_string(attrs, "alt")?,
        title: optional_string(attrs, "title")?,
        width: optional_string(attrs, "width")?,
        size: None,
    })
}

fn attachment_from_attrs(attrs: &Map<String, Value>) -> VaultResult<Resource> {
    Ok(Resource {
        source: required_string(attrs, "attachmentId", "附件")?,
        name: optional_string(attrs, "fileName")?,
        alt: None,
        title: None,
        width: None,
        size: optional_u64(attrs, "size")?,
    })
}

fn block_to_json(block: &Block) -> Value {
    match block {
        Block::Paragraph { content } => json_node("paragraph", None, inline_to_json(content)),
        Block::Blockquote { blocks } => json_node("blockquote", None, blocks_to_json(blocks)),
        Block::Details {
            open,
            summary,
            blocks,
        } => json_node(
            "details",
            Some(json!({"open": open})),
            vec![
                json_node("detailsSummary", None, inline_to_json(summary)),
                json_node("detailsContent", None, blocks_to_json(blocks)),
            ],
        ),
        Block::CodeBlock { language, text } => json_node(
            "codeBlock",
            Some(json!({"language": language})),
            if text.is_empty() {
                vec![]
            } else {
                vec![json!({"type": "text", "text": text})]
            },
        ),
        Block::HorizontalRule => json_node("horizontalRule", None, vec![]),
        Block::Heading { level, content } => json_node(
            "heading",
            Some(json!({ "level": level })),
            inline_to_json(content),
        ),
        Block::List { ordered, items } => json_node(
            if *ordered {
                "orderedList"
            } else {
                "bulletList"
            },
            None,
            items
                .iter()
                .map(|item| json_node("listItem", None, blocks_to_json(&item.blocks)))
                .collect(),
        ),
        Block::TaskList { items } => json_node(
            "taskList",
            None,
            items
                .iter()
                .map(|item| {
                    json_node(
                        "taskItem",
                        Some(json!({ "checked": item.checked })),
                        blocks_to_json(&item.blocks),
                    )
                })
                .collect(),
        ),
        Block::Table { rows } => json_node(
            "table",
            None,
            rows.iter()
                .map(|row| {
                    json_node(
                        "tableRow",
                        None,
                        row.cells
                            .iter()
                            .map(|cell| {
                                json_node(
                                    if cell.header {
                                        "tableHeader"
                                    } else {
                                        "tableCell"
                                    },
                                    Some(json!({"colspan": cell.colspan, "rowspan": cell.rowspan})),
                                    blocks_to_json(&cell.blocks),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        ),
        Block::Image { resource } => {
            json_node("image", Some(resource_to_attrs(resource)), Vec::new())
        }
        Block::Attachment { resource } => json_node(
            "attachmentBlock",
            Some(attachment_to_attrs(resource)),
            Vec::new(),
        ),
    }
}

fn blocks_to_json(blocks: &[Block]) -> Vec<Value> {
    blocks.iter().map(block_to_json).collect()
}

fn inline_to_json(content: &[Inline]) -> Vec<Value> {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, marks } => {
                let mut node = Map::new();
                node.insert("type".into(), Value::String("text".into()));
                node.insert("text".into(), Value::String(text.clone()));
                if !marks.is_empty() {
                    node.insert(
                        "marks".into(),
                        Value::Array(marks.iter().map(mark_to_json).collect()),
                    );
                }
                Value::Object(node)
            }
            Inline::HardBreak => json!({ "type": "hardBreak" }),
            Inline::Link { href, text } if href.starts_with("page://") => json!({
                "type": "pageLink",
                "attrs": { "pageId": href.trim_start_matches("page://"), "label": text },
            }),
            Inline::Link { href, text } => json!({
                "type": "text",
                "text": text,
                "marks": [{ "type": "link", "attrs": { "href": href } }],
            }),
        })
        .collect()
}

fn mark_to_json(mark: &TextMark) -> Value {
    match mark {
        TextMark::Bold => json!({ "type": "bold" }),
        TextMark::Italic => json!({ "type": "italic" }),
        TextMark::Underline => json!({ "type": "underline" }),
        TextMark::Strike => json!({ "type": "strike" }),
        TextMark::Code => json!({ "type": "code" }),
        TextMark::Highlight => json!({ "type": "highlight" }),
        TextMark::Link { href } => json!({ "type": "link", "attrs": { "href": href } }),
        TextMark::TextStyle { color, font_size } => {
            let mut attrs = Map::new();
            if let Some(color) = color {
                attrs.insert("color".into(), Value::String(color.clone()));
            }
            if let Some(font_size) = font_size {
                attrs.insert("fontSize".into(), Value::String(font_size.clone()));
            }
            json!({ "type": "textStyle", "attrs": attrs })
        }
    }
}

fn resource_to_attrs(resource: &Resource) -> Value {
    let mut attrs = Map::new();
    attrs.insert("src".into(), Value::String(resource.source.clone()));
    insert_optional(&mut attrs, "alt", &resource.alt);
    insert_optional(&mut attrs, "title", &resource.title);
    insert_optional(&mut attrs, "width", &resource.width);
    Value::Object(attrs)
}

fn attachment_to_attrs(resource: &Resource) -> Value {
    let mut attrs = Map::new();
    attrs.insert(
        "attachmentId".into(),
        Value::String(resource.source.clone()),
    );
    insert_optional(&mut attrs, "fileName", &resource.name);
    if let Some(size) = resource.size {
        attrs.insert("size".into(), Value::Number(size.into()));
    }
    Value::Object(attrs)
}

fn insert_optional(map: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        map.insert(key.into(), Value::String(value.clone()));
    }
}

fn json_node(kind: &str, attrs: Option<Value>, content: Vec<Value>) -> Value {
    let mut node = Map::new();
    node.insert("type".into(), Value::String(kind.into()));
    if let Some(attrs) = attrs {
        node.insert("attrs".into(), attrs);
    }
    if !content.is_empty() {
        node.insert("content".into(), Value::Array(content));
    }
    Value::Object(node)
}

fn object<'a>(value: &'a Value, subject: &str) -> VaultResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| VaultError::Validation(format!("{subject} 必须是对象")))
}

fn attrs(node: &Map<String, Value>) -> &Map<String, Value> {
    node.get("attrs")
        .and_then(Value::as_object)
        .unwrap_or(&EMPTY_MAP)
}

static EMPTY_MAP: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

fn contents(node: &Map<String, Value>) -> VaultResult<&[Value]> {
    match node.get("content") {
        Some(Value::Array(values)) => Ok(values),
        Some(_) => Err(VaultError::Validation("TipTap content 必须是数组".into())),
        None => Ok(&[]),
    }
}

fn string_field<'a>(node: &'a Map<String, Value>, key: &str) -> VaultResult<&'a str> {
    node.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| VaultError::Validation(format!("TipTap 节点缺少字符串字段 {key}")))
}

fn required_string(node: &Map<String, Value>, key: &str, subject: &str) -> VaultResult<String> {
    optional_string(node, key)?
        .ok_or_else(|| VaultError::Validation(format!("{subject} 缺少 {key}")))
}

fn optional_string(node: &Map<String, Value>, key: &str) -> VaultResult<Option<String>> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(VaultError::Validation(format!("{key} 必须是字符串"))),
    }
}

fn optional_u64(node: &Map<String, Value>, key: &str) -> VaultResult<Option<u64>> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| VaultError::Validation(format!("{key} 必须是非负整数"))),
    }
}

fn optional_bool(node: &Map<String, Value>, key: &str) -> VaultResult<Option<bool>> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(VaultError::Validation(format!("{key} 必须是布尔值"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PortableDocument {
        PortableDocument {
            blocks: vec![
                Block::Heading {
                    level: 2,
                    content: vec![Inline::Text {
                        text: "标题".into(),
                        marks: vec![TextMark::Bold],
                    }],
                },
                Block::Paragraph {
                    content: vec![
                        Inline::Text {
                            text: "带样式的链接 ".into(),
                            marks: vec![
                                TextMark::Italic,
                                TextMark::TextStyle {
                                    color: Some("#123456".into()),
                                    font_size: Some("18px".into()),
                                },
                            ],
                        },
                        Inline::Link {
                            href: "https://example.test".into(),
                            text: "外部链接".into(),
                        },
                        Inline::HardBreak,
                        Inline::Link {
                            href: "page://page-1".into(),
                            text: "内部页面".into(),
                        },
                    ],
                },
                Block::List {
                    ordered: false,
                    items: vec![ListItem {
                        blocks: vec![
                            Block::Paragraph {
                                content: vec![Inline::Text {
                                    text: "父项".into(),
                                    marks: vec![],
                                }],
                            },
                            Block::List {
                                ordered: true,
                                items: vec![ListItem {
                                    blocks: vec![Block::Paragraph {
                                        content: vec![Inline::Text {
                                            text: "子项".into(),
                                            marks: vec![],
                                        }],
                                    }],
                                }],
                            },
                        ],
                    }],
                },
                Block::TaskList {
                    items: vec![TaskItem {
                        checked: true,
                        blocks: vec![Block::Paragraph {
                            content: vec![Inline::Text {
                                text: "已完成待办".into(),
                                marks: vec![TextMark::Strike],
                            }],
                        }],
                    }],
                },
                Block::Table {
                    rows: vec![TableRow {
                        cells: vec![
                            TableCell {
                                header: true,
                                blocks: vec![Block::Paragraph {
                                    content: vec![Inline::Text {
                                        text: "列名".into(),
                                        marks: vec![],
                                    }],
                                }],
                                ..TableCell::default()
                            },
                            TableCell {
                                header: false,
                                blocks: vec![Block::Paragraph {
                                    content: vec![Inline::Text {
                                        text: "值".into(),
                                        marks: vec![],
                                    }],
                                }],
                                ..TableCell::default()
                            },
                        ],
                    }],
                },
                Block::Image {
                    resource: Resource {
                        source: "attachment://image-1".into(),
                        alt: Some("图片说明".into()),
                        width: Some("320px".into()),
                        ..Resource::default()
                    },
                },
                Block::Attachment {
                    resource: Resource {
                        source: "file-1".into(),
                        name: Some("报告.pdf".into()),
                        size: Some(42),
                        ..Resource::default()
                    },
                },
            ],
        }
    }

    #[test]
    fn representative_tiptap_document_round_trips_semantically() {
        let source = fixture();
        let json = source.to_tiptap_json();
        let restored = PortableDocument::from_tiptap_json(&json).unwrap();
        assert_eq!(restored, source);
    }

    #[test]
    fn details_round_trip_and_reject_malformed_structure() {
        let section = json!({"type": "details", "attrs": {"open": false}, "content": [
            {"type": "detailsSummary", "content": [{"type": "text", "text": "Title"}]},
            {"type": "detailsContent", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Body"}]}]}
        ]});
        let doc = json!({"type": "doc", "content": [section.clone()]});
        let portable = PortableDocument::from_tiptap_json(&doc).unwrap();
        assert_eq!(portable.to_tiptap_json(), doc);
        for children in [
            json!([]),
            json!([section]),
            json!([
                {"type": "detailsSummary"}, {"type": "detailsContent", "content": []}
            ]),
        ] {
            assert!(
                PortableDocument::from_tiptap_json(&json!({"type": "doc", "content": [
                    {"type": "details", "content": children}
                ]}))
                .is_err()
            );
        }
    }

    #[test]
    fn rejects_unknown_or_malformed_nodes_without_dropping_content() {
        let unsupported = json!({
            "type": "doc",
            "content": [{ "type": "drawingBlock", "attrs": { "svg": "<svg/>" } }],
        });
        assert!(matches!(
            PortableDocument::from_tiptap_json(&unsupported),
            Err(VaultError::Validation(_))
        ));
        let invalid_heading = json!({
            "type": "doc",
            "content": [{ "type": "heading", "attrs": { "level": 9 } }],
        });
        assert!(PortableDocument::from_tiptap_json(&invalid_heading).is_err());
    }
}
