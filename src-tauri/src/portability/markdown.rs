//! Markdown 与 `PortableDocument` 的受限双向转换。
//!
//! 这里故意只输出本应用能够再次读入的 Markdown 子集。资源路径经单独规划，
//! 因此调用方可以在复制附件前得到不冲突、不会逃逸导出目录的文件名。

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::document::{
    Block, Inline, ListItem, PortableDocument, Resource, TableCell, TableRow, TaskItem, TextMark,
};
use crate::error::{VaultError, VaultResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceReference {
    pub source: String,
    /// Relative path below the caller-selected resource directory.
    pub relative_path: String,
    pub is_image: bool,
}

/// Parse UTF-8 Markdown into the portable AST. Unknown constructs are retained as text where
/// possible instead of being executed or interpreted as HTML.
pub fn parse(input: &str) -> VaultResult<PortableDocument> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let events: Vec<_> = Parser::new_ext(input, options).collect();
    let mut cursor = 0;
    Ok(PortableDocument {
        blocks: parse_blocks(&events, &mut cursor, None)?,
    })
}

/// Render an interoperable Markdown subset. The result has no absolute resource path; use
/// [`collect_resources`] to plan and apply the resource copies alongside it.
pub fn render(document: &PortableDocument) -> String {
    let mut output = String::new();
    render_blocks(&document.blocks, &mut output, 0);
    output.trim_end().to_owned() + "\n"
}

/// Collects local image/attachment references and assigns deterministic, collision-free names.
/// Remote URLs and page links are not files and are deliberately excluded.
pub fn collect_resources(document: &PortableDocument) -> Vec<ResourceReference> {
    let mut resources = Vec::new();
    collect_blocks(&document.blocks, &mut resources);
    let mut names: HashMap<String, u32> = HashMap::new();
    let mut seen = BTreeMap::new();
    resources
        .into_iter()
        .filter(|resource| {
            seen.insert((resource.source.clone(), resource.is_image), ())
                .is_none()
        })
        .map(|mut resource| {
            let filename = resource_name(&resource.source, &resource.relative_path, &mut names);
            resource.relative_path = filename;
            resource
        })
        .collect()
}

fn parse_blocks<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    stop: Option<TagEnd>,
) -> VaultResult<Vec<Block>> {
    let mut blocks = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::End(end) if stop.as_ref() == Some(end) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::Paragraph) => {
                *cursor += 1;
                if let Some(Event::Start(Tag::Image {
                    dest_url, title, ..
                })) = events.get(*cursor)
                {
                    let image_end = events[*cursor..]
                        .iter()
                        .position(|event| matches!(event, Event::End(TagEnd::Image)));
                    if image_end.is_some_and(|offset| {
                        matches!(
                            events.get(*cursor + offset + 1),
                            Some(Event::End(TagEnd::Paragraph))
                        )
                    }) {
                        let source = dest_url.to_string();
                        let title = (!title.is_empty()).then(|| title.to_string());
                        *cursor += 1;
                        let alt = inline_text(events, cursor, TagEnd::Image);
                        *cursor += 1;
                        blocks.push(Block::Image {
                            resource: Resource {
                                source,
                                title,
                                alt: (!alt.is_empty()).then_some(alt),
                                ..Resource::default()
                            },
                        });
                        continue;
                    }
                }
                blocks.push(Block::Paragraph {
                    content: parse_inlines(events, cursor, TagEnd::Paragraph, Vec::new())?,
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let heading_level = *level;
                let level = heading_level as u8;
                *cursor += 1;
                blocks.push(Block::Heading {
                    level,
                    content: parse_inlines(
                        events,
                        cursor,
                        TagEnd::Heading(heading_level),
                        Vec::new(),
                    )?,
                });
            }
            Event::Start(Tag::List(start)) => {
                let ordered = start.is_some();
                *cursor += 1;
                let items = parse_list(events, cursor)?;
                if !ordered
                    && !items.is_empty()
                    && items.iter().all(|(_, checked)| checked.is_some())
                {
                    blocks.push(Block::TaskList {
                        items: items
                            .into_iter()
                            .map(|(item, checked)| TaskItem {
                                checked: checked.unwrap_or(false),
                                blocks: item.blocks,
                            })
                            .collect(),
                    });
                } else {
                    blocks.push(Block::List {
                        ordered,
                        items: items.into_iter().map(|(item, _)| item).collect(),
                    });
                }
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let end = TagEnd::BlockQuote(*kind);
                *cursor += 1;
                blocks.push(Block::Blockquote {
                    blocks: parse_blocks(events, cursor, Some(end))?,
                });
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_owned)
                    }
                    CodeBlockKind::Indented => None,
                };
                *cursor += 1;
                let mut text = String::new();
                while *cursor < events.len() {
                    match &events[*cursor] {
                        Event::End(TagEnd::CodeBlock) => {
                            *cursor += 1;
                            break;
                        }
                        Event::Text(value) => text.push_str(value),
                        _ => {}
                    }
                    *cursor += 1;
                }
                // Fences contribute one delimiter newline, separate from code content.
                if text.ends_with('\n') {
                    text.pop();
                }
                blocks.push(Block::CodeBlock { language, text });
            }
            Event::Start(Tag::Table(_)) => {
                *cursor += 1;
                blocks.push(Block::Table {
                    rows: parse_table(events, cursor)?,
                });
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                let source = dest_url.to_string();
                let title = (!title.is_empty()).then(|| title.to_string());
                *cursor += 1;
                let alt = inline_text(events, cursor, TagEnd::Image);
                blocks.push(Block::Image {
                    resource: Resource {
                        source,
                        alt: (!alt.is_empty()).then_some(alt),
                        title,
                        ..Resource::default()
                    },
                });
            }
            Event::Rule => {
                blocks.push(Block::HorizontalRule);
                *cursor += 1;
            }
            Event::Text(text) | Event::Code(text) => {
                blocks.push(Block::Paragraph {
                    content: vec![Inline::Text {
                        text: text.to_string(),
                        marks: vec![],
                    }],
                });
                *cursor += 1;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let mut source = html.to_string();
                *cursor += 1;
                while let Some(Event::Html(value)) = events.get(*cursor) {
                    source.push_str(value);
                    *cursor += 1;
                }
                blocks.extend(super::html::parse(&source)?.blocks);
            }
            Event::End(_) => {
                *cursor += 1;
            }
            _ => {
                *cursor += 1;
            }
        }
    }
    Ok(blocks)
}

fn parse_list<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
) -> VaultResult<Vec<(ListItem, Option<bool>)>> {
    let mut items = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::End(TagEnd::List(_)) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::Item) => {
                *cursor += 1;
                let checked = match events.get(*cursor) {
                    Some(Event::TaskListMarker(checked)) => {
                        *cursor += 1;
                        Some(*checked)
                    }
                    _ => None,
                };
                let blocks = parse_blocks(events, cursor, Some(TagEnd::Item))?;
                items.push((ListItem { blocks }, checked));
            }
            _ => *cursor += 1,
        }
    }
    Ok(items)
}

fn parse_table<'a>(events: &[Event<'a>], cursor: &mut usize) -> VaultResult<Vec<TableRow>> {
    let mut rows = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::End(TagEnd::Table) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                let header = matches!(&events[*cursor], Event::Start(Tag::TableHead));
                let end = if header {
                    TagEnd::TableHead
                } else {
                    TagEnd::TableRow
                };
                *cursor += 1;
                let mut cells = Vec::new();
                while *cursor < events.len() {
                    match &events[*cursor] {
                        Event::End(tag) if *tag == end => {
                            *cursor += 1;
                            break;
                        }
                        Event::Start(Tag::TableCell) => {
                            *cursor += 1;
                            let content =
                                parse_inlines(events, cursor, TagEnd::TableCell, Vec::new())?;
                            cells.push(TableCell {
                                header,
                                blocks: vec![Block::Paragraph { content }],
                                ..TableCell::default()
                            });
                        }
                        _ => *cursor += 1,
                    }
                }
                rows.push(TableRow { cells });
            }
            _ => *cursor += 1,
        }
    }
    Ok(rows)
}

fn parse_inlines<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    stop: TagEnd,
    marks: Vec<TextMark>,
) -> VaultResult<Vec<Inline>> {
    let mut inlines = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::End(end) if *end == stop => {
                *cursor += 1;
                break;
            }
            Event::Text(text) => {
                inlines.push(Inline::Text {
                    text: text.to_string(),
                    marks: marks.clone(),
                });
                *cursor += 1;
            }
            Event::Code(text) => {
                let mut value = marks.clone();
                value.push(TextMark::Code);
                inlines.push(Inline::Text {
                    text: text.to_string(),
                    marks: value,
                });
                *cursor += 1;
            }
            Event::SoftBreak | Event::HardBreak => {
                inlines.push(Inline::HardBreak);
                *cursor += 1;
            }
            Event::Start(Tag::Emphasis) => {
                *cursor += 1;
                let mut next = marks.clone();
                next.push(TextMark::Italic);
                inlines.extend(parse_inlines(events, cursor, TagEnd::Emphasis, next)?);
            }
            Event::Start(Tag::Strong) => {
                *cursor += 1;
                let mut next = marks.clone();
                next.push(TextMark::Bold);
                inlines.extend(parse_inlines(events, cursor, TagEnd::Strong, next)?);
            }
            Event::Start(Tag::Strikethrough) => {
                *cursor += 1;
                let mut next = marks.clone();
                next.push(TextMark::Strike);
                inlines.extend(parse_inlines(events, cursor, TagEnd::Strikethrough, next)?);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let href = dest_url.to_string();
                *cursor += 1;
                let text = inline_text(events, cursor, TagEnd::Link);
                inlines.push(Inline::Link { href, text });
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                let source = dest_url.to_string();
                let title = (!title.is_empty()).then(|| title.to_string());
                *cursor += 1;
                let alt = inline_text(events, cursor, TagEnd::Image);
                inlines.push(Inline::Text {
                    text: format!(
                        "![{}]({}{})",
                        alt,
                        source,
                        title.map(|v| format!(" \\\"{}\\\"", v)).unwrap_or_default()
                    ),
                    marks: marks.clone(),
                });
            }
            Event::Html(value) | Event::InlineHtml(value) => {
                inlines.push(Inline::Text {
                    text: value.to_string(),
                    marks: marks.clone(),
                });
                *cursor += 1;
            }
            Event::Start(_) => {
                *cursor += 1;
            }
            _ => {
                *cursor += 1;
            }
        }
    }
    Ok(inlines)
}

fn inline_text<'a>(events: &[Event<'a>], cursor: &mut usize, stop: TagEnd) -> String {
    let mut text = String::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::End(end) if *end == stop => {
                *cursor += 1;
                break;
            }
            Event::Text(value) | Event::Code(value) => text.push_str(value),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
        *cursor += 1;
    }
    text
}

fn render_blocks(blocks: &[Block], out: &mut String, depth: usize) {
    for block in blocks {
        match block {
            Block::Blockquote { blocks } => {
                let mut quote = String::new();
                render_blocks(blocks, &mut quote, depth);
                for line in quote.trim_end().lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::CodeBlock { language, text } => {
                let longest = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                out.push_str(&fence);
                out.push_str(language.as_deref().unwrap_or(""));
                out.push('\n');
                out.push_str(text);
                out.push('\n');
                out.push_str(&fence);
                out.push_str("\n\n");
            }
            Block::HorizontalRule => out.push_str("---\n\n"),
            Block::Paragraph { content } => {
                render_inlines(content, out);
                out.push_str("\n\n");
            }
            Block::Heading { level, content } => {
                out.push_str(&"#".repeat((*level).into()));
                out.push(' ');
                render_inlines(content, out);
                out.push_str("\n\n");
            }
            Block::List { ordered, items } => render_list(items, *ordered, out, depth),
            Block::TaskList { items } => {
                for item in items {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str(if item.checked { "- [x] " } else { "- [ ] " });
                    render_blocks_inline(&item.blocks, out);
                    out.push('\n');
                }
            }
            Block::Table { rows } => render_table(rows, out),
            Block::Image { resource } => {
                out.push_str("![");
                out.push_str(resource.alt.as_deref().unwrap_or(""));
                out.push_str("](");
                out.push_str(&resource.source);
                if let Some(title) = &resource.title {
                    out.push_str(" \\");
                    out.push_str(title);
                    out.push('\"');
                }
                out.push_str(")\n\n");
            }
            Block::Attachment { resource } => {
                out.push('[');
                out.push_str(resource.name.as_deref().unwrap_or(&resource.source));
                out.push_str("](");
                out.push_str(&resource.source);
                out.push_str(")\n\n");
            }
        }
    }
}

fn render_list(items: &[ListItem], ordered: bool, out: &mut String, depth: usize) {
    for (index, item) in items.iter().enumerate() {
        out.push_str(&"  ".repeat(depth));
        if ordered {
            out.push_str(&(index + 1).to_string());
            out.push_str(". ");
        } else {
            out.push_str("- ");
        }
        render_blocks_inline(&item.blocks, out);
        out.push('\n');
    }
    if depth == 0 {
        out.push('\n');
    }
}

fn render_blocks_inline(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph { content } | Block::Heading { content, .. } => {
                render_inlines(content, out)
            }
            Block::List { ordered, items } => {
                out.push('\n');
                render_list(items, *ordered, out, 1);
            }
            Block::TaskList { items } => {
                for item in items {
                    out.push_str("- [");
                    out.push(if item.checked { 'x' } else { ' ' });
                    out.push_str("] ");
                    render_blocks_inline(&item.blocks, out);
                }
            }
            other => {
                let mut nested = String::new();
                render_blocks(std::slice::from_ref(other), &mut nested, 1);
                out.push('\n');
                for line in nested.trim_end().lines() {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
}

fn render_inlines(content: &[Inline], out: &mut String) {
    for inline in content {
        match inline {
            Inline::HardBreak => out.push_str("  \n"),
            Inline::Link { href, text } => {
                out.push('[');
                out.push_str(text);
                out.push_str("](");
                out.push_str(href);
                out.push(')');
            }
            Inline::Text { text, marks } => {
                let mut value = text
                    .replace('\\', "\\\\")
                    .replace('*', "\\*")
                    .replace('_', "\\_");
                for mark in marks {
                    value = match mark {
                        TextMark::Bold => format!("**{value}**"),
                        TextMark::Italic => format!("*{value}*"),
                        TextMark::Strike => format!("~~{value}~~"),
                        TextMark::Code => format!("`{value}`"),
                        _ => value,
                    };
                }
                out.push_str(&value);
            }
        }
    }
}

fn render_table(rows: &[TableRow], out: &mut String) {
    if rows.is_empty() {
        return;
    }
    if rows.iter().enumerate().any(|(index, row)| {
        row.cells.iter().any(|cell| {
            cell.colspan > 1
                || cell.rowspan > 1
                || cell.header != (index == 0)
                || !matches!(cell.blocks.as_slice(), [Block::Paragraph { .. }])
        })
    }) {
        out.push_str(&super::html::render(&PortableDocument {
            blocks: vec![Block::Table {
                rows: rows.to_vec(),
            }],
        }));
        out.push_str("\n\n");
        return;
    }
    for (row_index, row) in rows.iter().enumerate() {
        out.push('|');
        for cell in &row.cells {
            out.push(' ');
            render_blocks_inline(&cell.blocks, out);
            out.push_str(" |");
        }
        out.push('\n');
        if row_index == 0 {
            out.push('|');
            for _ in &row.cells {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out.push('\n');
}

fn collect_blocks(blocks: &[Block], into: &mut Vec<ResourceReference>) {
    for block in blocks {
        match block {
            Block::Blockquote { blocks } => collect_blocks(blocks, into),
            Block::Image { resource } => collect_resource(resource, true, into),
            Block::Attachment { resource } => collect_resource(resource, false, into),
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.blocks, into);
                }
            }
            Block::TaskList { items } => {
                for item in items {
                    collect_blocks(&item.blocks, into);
                }
            }
            Block::Table { rows } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_blocks(&cell.blocks, into);
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_resource(resource: &Resource, is_image: bool, into: &mut Vec<ResourceReference>) {
    if !resource.source.starts_with("http://")
        && !resource.source.starts_with("https://")
        && !resource.source.starts_with("page://")
    {
        into.push(ResourceReference {
            source: resource.source.clone(),
            relative_path: resource
                .name
                .clone()
                .unwrap_or_else(|| resource.source.clone()),
            is_image,
        });
    }
}

fn resource_name(source: &str, requested: &str, names: &mut HashMap<String, u32>) -> String {
    let candidate = Path::new(requested)
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(source);
    let mut safe: String = candidate
        .chars()
        .map(|ch| {
            if ch.is_control()
                || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '\"' | '<' | '>' | '|')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if safe.is_empty() || safe == "." || safe == ".." {
        safe = "resource".into();
    }
    let count = names.entry(safe.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        return safe;
    }
    let path = Path::new(&safe);
    let stem = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or("resource");
    let extension = path
        .extension()
        .and_then(|part| part.to_str())
        .map(|part| format!(".{part}"))
        .unwrap_or_default();
    format!("{stem}-{}{extension}", count)
}

/// Validate that an output resource path is relative and has no traversal component.
pub fn checked_relative_path(path: &str) -> VaultResult<&Path> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(VaultError::Validation("资源路径必须是安全相对路径".into()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_blocks_and_html_merged_tables_survive_import_export() {
        let input = serde_json::json!({"type":"doc","content":[
            {"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"Quoted"}]}]},
            {"type":"codeBlock","attrs":{"language":"markdown"},"content":[{"type":"text","text":"```js\nconst n = 1;\n```\n"}]},
            {"type":"horizontalRule"},
            {"type":"table","content":[
                {"type":"tableRow","content":[
                    {"type":"tableHeader","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"A & B","marks":[{"type":"bold"}]}]}]},
                    {"type":"tableCell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]},{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]}
                ]},
                {"type":"tableRow","content":[
                    {"type":"tableCell","content":[{"type":"paragraph","content":[{"type":"text","text":"C"}]}]},
                    {"type":"tableCell","content":[{"type":"paragraph","content":[{"type":"text","text":"D"}]}]}
                ]}
            ]}
        ]});
        let document = PortableDocument::from_tiptap_json(&input).unwrap();
        let source = render(&document);
        assert!(source.contains("> Quoted"));
        assert!(source.contains("````markdown"));
        assert!(source.contains("<table>"));
        assert!(source.contains("rowspan=\"2\""));
        assert!(source.contains("colspan=\"2\""));
        assert_eq!(parse(&source).unwrap(), document);
        let html = super::super::html::render(&document);
        assert_eq!(super::super::html::parse(&html).unwrap(), document);
        assert_eq!(
            PortableDocument::from_tiptap_json(&document.to_tiptap_json()).unwrap(),
            document
        );
    }

    #[test]
    fn ordinary_tables_stay_gfm_and_headerless_tables_use_html() {
        let ordinary = parse("| A | B |\n| --- | --- |\n| C | D |\n").unwrap();
        assert!(render(&ordinary).starts_with("| A | B |"));
        let headerless =
            parse("<table><tr><td><strong>A</strong></td><td>B</td></tr></table>").unwrap();
        let source = render(&headerless);
        assert!(source.starts_with("<table>"));
        assert!(source.contains("<strong>A</strong>"));
        assert_eq!(parse(&source).unwrap(), headerless);
    }

    #[test]
    fn resources_inside_quotes_are_still_collected() {
        let document = parse("> ![photo](photo.png)\n").unwrap();
        assert_eq!(collect_resources(&document).len(), 1);
    }

    #[test]
    fn markdown_round_trip_keeps_supported_structure() {
        let input = "# 标题\n\n- 父项\n  1. 子项\n\n- [x] 已完成\n\n| 名称 | 值 |\n| --- | --- |\n| a | b |\n\n[链接](https://example.test)\n\n![图](photo.png)\n";
        let parsed = parse(input).unwrap();
        let rendered = render(&parsed);
        let restored = parse(&rendered).unwrap();
        assert!(!restored.blocks.is_empty());
        let task_document = parse("- [x] 完成\n- [ ] 待办\n").unwrap();
        assert!(matches!(
            task_document.blocks.first(),
            Some(Block::TaskList { .. })
        ));
        assert!(rendered.contains("标题"));
        assert!(rendered.contains("photo.png"));
    }

    #[test]
    fn resource_names_are_safe_and_collision_free() {
        let document = PortableDocument {
            blocks: vec![
                Block::Attachment {
                    resource: Resource {
                        source: "attachment://1".into(),
                        name: Some("../same.pdf".into()),
                        ..Resource::default()
                    },
                },
                Block::Attachment {
                    resource: Resource {
                        source: "attachment://2".into(),
                        name: Some("same.pdf".into()),
                        ..Resource::default()
                    },
                },
            ],
        };
        let resources = collect_resources(&document);
        assert_eq!(resources[0].relative_path, "same.pdf");
        assert_eq!(resources[1].relative_path, "same-2.pdf");
        assert!(checked_relative_path(&resources[0].relative_path).is_ok());
    }
}
