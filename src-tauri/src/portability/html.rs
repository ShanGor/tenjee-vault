//! A deliberately small HTML allow-list boundary for portable documents.
//!
//! Input is first sanitized by `ammonia`; the parser below only maps the resulting
//! structural subset to `PortableDocument`. It never evaluates HTML, CSS or URLs.

use ammonia::Builder;

use super::document::{
    Block, Inline, ListItem, PortableDocument, Resource, TableCell, TableRow, TextMark,
};
use crate::error::{VaultError, VaultResult};

/// Remove active HTML before parsing. Remote active content (iframe/object/embed and remote
/// image loads) is dropped, even if a browser would otherwise display it safely.
pub fn sanitize(input: &str) -> String {
    let cleaned = Builder::default().clean(input).to_string();
    remove_remote_images(&cleaned)
}

pub fn parse(input: &str) -> VaultResult<PortableDocument> {
    let safe = sanitize(input);
    let tokens = tokenize(&safe)?;
    let mut cursor = 0;
    Ok(PortableDocument {
        blocks: parse_blocks(&tokens, &mut cursor, None),
    })
}

pub fn render(document: &PortableDocument) -> String {
    let mut out = String::new();
    render_blocks(&document.blocks, &mut out);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

fn tokenize(input: &str) -> VaultResult<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut rest = input;
    while let Some(index) = rest.find('<') {
        if index > 0 {
            tokens.push(Token::Text(unescape(&rest[..index])));
        }
        let end = rest[index..]
            .find('>')
            .ok_or_else(|| VaultError::Validation("HTML 标签未闭合".into()))?
            + index;
        let raw = rest[index + 1..end].trim();
        rest = &rest[end + 1..];
        if raw.starts_with('!') {
            continue;
        }
        if let Some(name) = raw.strip_prefix('/') {
            tokens.push(Token::End(name.trim().to_ascii_lowercase()));
            continue;
        }
        let self_closing = raw.ends_with('/');
        let raw = raw.trim_end_matches('/').trim();
        let (name, attributes) = tag_parts(raw);
        if name.is_empty() {
            continue;
        }
        let name = name.to_ascii_lowercase();
        tokens.push(Token::Start(name.clone(), attributes));
        if self_closing || matches!(name.as_str(), "br" | "img" | "input" | "hr") {
            tokens.push(Token::End(name));
        }
    }
    if !rest.is_empty() {
        tokens.push(Token::Text(unescape(rest)));
    }
    Ok(tokens)
}

fn tag_parts(raw: &str) -> (String, Vec<(String, String)>) {
    let mut parts = raw.split_whitespace();
    let name = parts.next().unwrap_or_default().to_owned();
    let mut attributes = Vec::new();
    for part in parts {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.to_ascii_lowercase();
        if key.starts_with("on") || key == "style" {
            continue;
        }
        attributes.push((key, value.trim_matches(['\'', '\"']).to_owned()));
    }
    (name, attributes)
}

fn parse_blocks(tokens: &[Token], cursor: &mut usize, end: Option<&str>) -> Vec<Block> {
    let mut blocks = Vec::new();
    while *cursor < tokens.len() {
        match &tokens[*cursor] {
            Token::End(name) if Some(name.as_str()) == end => {
                *cursor += 1;
                break;
            }
            Token::Start(name, _)
                if matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") =>
            {
                let level = name[1..].parse().unwrap_or(1);
                *cursor += 1;
                blocks.push(Block::Heading {
                    level,
                    content: parse_inlines(tokens, cursor, Some(name)),
                });
            }
            Token::Start(name, _) if name == "p" || name == "div" => {
                *cursor += 1;
                blocks.push(Block::Paragraph {
                    content: parse_inlines(tokens, cursor, Some(name)),
                });
            }
            Token::Start(name, _) if name == "ul" || name == "ol" => {
                let ordered = name == "ol";
                *cursor += 1;
                let mut items = Vec::new();
                while *cursor < tokens.len() {
                    match &tokens[*cursor] {
                        Token::End(close) if close == name => {
                            *cursor += 1;
                            break;
                        }
                        Token::Start(item, _) if item == "li" => {
                            *cursor += 1;
                            let content = parse_inlines(tokens, cursor, Some(item));
                            items.push(ListItem {
                                blocks: vec![Block::Paragraph { content }],
                            });
                        }
                        _ => *cursor += 1,
                    }
                }
                blocks.push(Block::List { ordered, items });
            }
            Token::Start(name, _) if name == "table" => {
                *cursor += 1;
                blocks.push(Block::Table {
                    rows: parse_table(tokens, cursor),
                });
            }
            Token::Start(name, attrs) if name == "img" => {
                let resource = Resource {
                    source: attr(attrs, "src").unwrap_or_default(),
                    alt: attr(attrs, "alt"),
                    title: attr(attrs, "title"),
                    ..Resource::default()
                };
                *cursor += 1;
                consume_end(tokens, cursor, name);
                blocks.push(Block::Image { resource });
            }
            Token::Text(text) if !text.trim().is_empty() => {
                blocks.push(Block::Paragraph {
                    content: vec![Inline::Text {
                        text: text.clone(),
                        marks: vec![],
                    }],
                });
                *cursor += 1;
            }
            _ => *cursor += 1,
        }
    }
    blocks
}

fn parse_table(tokens: &[Token], cursor: &mut usize) -> Vec<TableRow> {
    let mut rows = Vec::new();
    while *cursor < tokens.len() {
        match &tokens[*cursor] {
            Token::End(name) if name == "table" => {
                *cursor += 1;
                break;
            }
            Token::Start(name, _) if name == "tr" => {
                *cursor += 1;
                let mut cells = Vec::new();
                while *cursor < tokens.len() {
                    match &tokens[*cursor] {
                        Token::End(close) if close == "tr" => {
                            *cursor += 1;
                            break;
                        }
                        Token::Start(kind, _) if kind == "th" || kind == "td" => {
                            let header = kind == "th";
                            *cursor += 1;
                            let content = parse_inlines(tokens, cursor, Some(kind));
                            cells.push(TableCell {
                                header,
                                blocks: vec![Block::Paragraph { content }],
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
    rows
}

fn parse_inlines(tokens: &[Token], cursor: &mut usize, end: Option<&str>) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut marks = Vec::new();
    while *cursor < tokens.len() {
        match &tokens[*cursor] {
            Token::End(name) if Some(name.as_str()) == end => {
                *cursor += 1;
                break;
            }
            Token::Text(text) => {
                out.push(Inline::Text {
                    text: text.clone(),
                    marks: marks.clone(),
                });
                *cursor += 1;
            }
            Token::Start(name, attrs) if name == "br" => {
                out.push(Inline::HardBreak);
                *cursor += 1;
                consume_end(tokens, cursor, name);
            }
            Token::Start(name, _) if name == "strong" || name == "b" => {
                marks.push(TextMark::Bold);
                *cursor += 1;
            }
            Token::Start(name, _) if name == "em" || name == "i" => {
                marks.push(TextMark::Italic);
                *cursor += 1;
            }
            Token::Start(name, _) if name == "s" || name == "del" || name == "strike" => {
                marks.push(TextMark::Strike);
                *cursor += 1;
            }
            Token::Start(name, _) if name == "code" => {
                marks.push(TextMark::Code);
                *cursor += 1;
            }
            Token::End(name)
                if matches!(
                    name.as_str(),
                    "strong" | "b" | "em" | "i" | "s" | "del" | "strike" | "code"
                ) =>
            {
                marks.pop();
                *cursor += 1;
            }
            Token::Start(name, attrs) if name == "a" => {
                let href = attr(attrs, "href")
                    .filter(|url| safe_href(url))
                    .unwrap_or_default();
                *cursor += 1;
                let label = inline_text(tokens, cursor, "a");
                out.push(Inline::Link { href, text: label });
            }
            Token::Start(name, attrs) if name == "img" => {
                let src = attr(attrs, "src").unwrap_or_default();
                let alt = attr(attrs, "alt").unwrap_or_default();
                out.push(Inline::Text {
                    text: format!("![{alt}]({src})"),
                    marks: marks.clone(),
                });
                *cursor += 1;
                consume_end(tokens, cursor, name);
            }
            Token::Start(_, _) => *cursor += 1,
            Token::End(_) => *cursor += 1,
        }
    }
    out
}

fn inline_text(tokens: &[Token], cursor: &mut usize, end: &str) -> String {
    let mut value = String::new();
    while *cursor < tokens.len() {
        match &tokens[*cursor] {
            Token::End(name) if name == end => {
                *cursor += 1;
                break;
            }
            Token::Text(text) => value.push_str(text),
            _ => {}
        }
        *cursor += 1;
    }
    value
}

fn consume_end(tokens: &[Token], cursor: &mut usize, name: &str) {
    if matches!(tokens.get(*cursor), Some(Token::End(end)) if end == name) {
        *cursor += 1;
    }
}
fn attr(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}
fn safe_href(url: &str) -> bool {
    !url.trim().to_ascii_lowercase().starts_with("javascript:")
        && !url.trim().to_ascii_lowercase().starts_with("data:")
}

fn remove_remote_images(html: &str) -> String {
    let mut output = String::new();
    let mut rest = html;
    while let Some(index) = rest.find("<img") {
        output.push_str(&rest[..index]);
        let Some(end) = rest[index..].find('>') else {
            break;
        };
        let tag = &rest[index..index + end + 1];
        let lower = tag.to_ascii_lowercase();
        if !(lower.contains("src=\"http://")
            || lower.contains("src=\"https://")
            || lower.contains("src='http://")
            || lower.contains("src='https://"))
        {
            output.push_str(tag);
        }
        rest = &rest[index + end + 1..];
    }
    output.push_str(rest);
    output
}

fn render_blocks(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph { content } => {
                out.push_str("<p>");
                render_inlines(content, out);
                out.push_str("</p>");
            }
            Block::Heading { level, content } => {
                out.push_str(&format!("<h{level}>"));
                render_inlines(content, out);
                out.push_str(&format!("</h{level}>"));
            }
            Block::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                out.push('<');
                out.push_str(tag);
                out.push('>');
                for item in items {
                    out.push_str("<li>");
                    render_blocks(&item.blocks, out);
                    out.push_str("</li>");
                }
                out.push_str(&format!("</{tag}>"));
            }
            Block::TaskList { items } => {
                out.push_str("<ul>");
                for item in items {
                    out.push_str("<li><input type=\"checkbox\" disabled");
                    if item.checked {
                        out.push_str(" checked");
                    }
                    out.push_str(">");
                    render_blocks(&item.blocks, out);
                    out.push_str("</li>");
                }
                out.push_str("</ul>");
            }
            Block::Table { rows } => {
                out.push_str("<table>");
                for row in rows {
                    out.push_str("<tr>");
                    for cell in &row.cells {
                        let tag = if cell.header { "th" } else { "td" };
                        out.push('<');
                        out.push_str(tag);
                        out.push('>');
                        render_blocks(&cell.blocks, out);
                        out.push_str(&format!("</{tag}>"));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</table>");
            }
            Block::Image { resource } => {
                out.push_str("<img src=\"");
                out.push_str(&escape_attr(&resource.source));
                out.push_str("\" alt=\"");
                out.push_str(&escape_attr(resource.alt.as_deref().unwrap_or("")));
                out.push_str("\">");
            }
            Block::Attachment { resource } => {
                out.push_str("<a href=\"");
                out.push_str(&escape_attr(&resource.source));
                out.push_str("\">");
                out.push_str(&escape(
                    resource.name.as_deref().unwrap_or(&resource.source),
                ));
                out.push_str("</a>");
            }
        }
    }
}

fn render_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::HardBreak => out.push_str("<br>"),
            Inline::Link { href, text } => {
                out.push_str("<a href=\"");
                out.push_str(&escape_attr(href));
                out.push_str("\">");
                out.push_str(&escape(text));
                out.push_str("</a>");
            }
            Inline::Text { text, marks } => {
                let mut value = escape(text);
                for mark in marks.iter().rev() {
                    let tag = match mark {
                        TextMark::Bold => Some("strong"),
                        TextMark::Italic => Some("em"),
                        TextMark::Strike => Some("s"),
                        TextMark::Code => Some("code"),
                        _ => None,
                    };
                    if let Some(tag) = tag {
                        value = format!("<{tag}>{value}</{tag}>");
                    }
                }
                out.push_str(&value);
            }
        }
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn escape_attr(value: &str) -> String {
    escape(value).replace('\"', "&quot;")
}
fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_active_content_and_keeps_editable_nodes() {
        let source = "<h1 onclick=\"alert(1)\">标题</h1><p><strong>正文</strong><script>alert(1)</script><a href=\"javascript:alert(1)\">坏链接</a></p><img src=\"https://tracker.test/a.png\">";
        let safe = sanitize(source);
        assert!(!safe.contains("script"));
        assert!(!safe.contains("onclick"));
        assert!(!safe.contains("tracker.test"));
        let document = parse(source).unwrap();
        assert!(matches!(
            document.blocks.first(),
            Some(Block::Heading { .. })
        ));
        assert!(render(&document).contains("标题"));
    }
}
