// Meeting-minutes .docx export: converts the minutes Markdown produced by
// the LLM into a minimal WordprocessingML package. Only the constructs the
// minutes prompt asks for are rendered (headings, bullet/numbered lists,
// bold, plain paragraphs); tables are rendered as plain text lines.

use std::io::Write;

use crate::errors::AppError;

/// One styled text span inside a paragraph.
struct Run {
    text: String,
    bold: bool,
    /// Font size in half-points (Word's `w:sz` unit).
    size: u16,
}

/// One block-level element parsed from a Markdown line.
enum Block {
    /// Heading level 2-4 (level 1 is the document title, already rendered).
    Heading(u8, Vec<Run>),
    Paragraph(Vec<Run>),
    /// Bullet item; the marker glyph is part of the text.
    Bullet(Vec<Run>),
    /// Numbered item; the original "3." marker is kept in the text.
    Numbered(Vec<Run>),
}

const BODY_SIZE: u16 = 22; // 11pt
const H2_SIZE: u16 = 32; // 16pt
const H3_SIZE: u16 = 26; // 13pt

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Split a line into runs on `**bold**` markers.
fn parse_inline(text: &str, size: u16, bold: bool) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut current_bold = bold;
    let mut buffer = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find("**") {
        buffer.push_str(&rest[..pos]);
        if !buffer.is_empty() {
            runs.push(Run {
                text: std::mem::take(&mut buffer),
                bold: current_bold,
                size,
            });
        }
        current_bold = !current_bold;
        rest = &rest[pos + 2..];
    }
    buffer.push_str(rest);
    if !buffer.is_empty() {
        runs.push(Run {
            text: buffer,
            bold: current_bold,
            size,
        });
    }
    runs
}

/// Parse minutes Markdown into renderable blocks. Lines inside fenced code
/// blocks, horizontal rules, and level-1 headings (the document title) are
/// skipped; table rows fall back to plain paragraphs.
fn parse_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    for raw in markdown.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.is_empty() || trimmed == "---" || trimmed == "***" {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            blocks.push(Block::Heading(3, parse_inline(rest, H3_SIZE, true)));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            blocks.push(Block::Heading(2, parse_inline(rest, H2_SIZE, true)));
        } else if trimmed.starts_with("# ") {
            // Level-1 heading is the document title, already rendered.
            continue;
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let mut runs = parse_inline(rest, BODY_SIZE, false);
            if let Some(first) = runs.first_mut() {
                first.text = format!("• {}", first.text);
            } else {
                runs.push(Run {
                    text: "•".into(),
                    bold: false,
                    size: BODY_SIZE,
                });
            }
            blocks.push(Block::Bullet(runs));
        } else if trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
            && trimmed.contains(". ")
            && trimmed.split(". ").next().is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            blocks.push(Block::Numbered(parse_inline(trimmed, BODY_SIZE, false)));
        } else {
            blocks.push(Block::Paragraph(parse_inline(trimmed, BODY_SIZE, false)));
        }
    }
    blocks
}

fn run_xml(run: &Run) -> String {
    let mut props = String::new();
    if run.bold {
        props.push_str("<w:b/>");
    }
    props.push_str(&format!(
        "<w:sz w:val=\"{}\"/><w:szCs w:val=\"{}\"/>",
        run.size, run.size
    ));
    format!(
        "<w:r><w:rPr>{}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        props,
        xml_escape(&run.text)
    )
}

fn paragraph_xml(runs: &[Run], indent: u16, before: u16, after: u16, center: bool) -> String {
    let mut props = String::new();
    if center {
        props.push_str("<w:jc w:val=\"center\"/>");
    }
    if indent > 0 {
        props.push_str(&format!("<w:ind w:left=\"{}\"/>", indent));
    }
    if before > 0 || after > 0 {
        props.push_str(&format!(
            "<w:spacing w:before=\"{}\" w:after=\"{}\"/>",
            before, after
        ));
    }
    let body: String = runs.iter().map(run_xml).collect();
    format!(
        "<w:p>{}{}</w:p>",
        if props.is_empty() {
            String::new()
        } else {
            format!("<w:pPr>{}</w:pPr>", props)
        },
        body
    )
}

/// Render the minutes document body (paragraph XML only, no envelope).
fn minutes_body_xml(title: &str, markdown: &str) -> String {
    let mut parts = Vec::new();
    // Document title from the recording name, centered and spaced out.
    parts.push(paragraph_xml(
        &[Run {
            text: title.to_string(),
            bold: true,
            size: 44, // 22pt
        }],
        0,
        0,
        240,
        true,
    ));
    for block in parse_blocks(markdown) {
        match block {
            Block::Heading(2, runs) => parts.push(paragraph_xml(&runs, 0, 240, 120, false)),
            Block::Heading(_, runs) => parts.push(paragraph_xml(&runs, 0, 180, 100, false)),
            Block::Bullet(runs) | Block::Numbered(runs) => {
                parts.push(paragraph_xml(&runs, 360, 40, 40, false))
            }
            Block::Paragraph(runs) => parts.push(paragraph_xml(&runs, 0, 60, 60, false)),
        }
    }
    parts.concat()
}

/// Assemble the complete .docx package bytes for the given minutes.
pub fn minutes_docx_bytes(title: &str, markdown: &str) -> Result<Vec<u8>, AppError> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:body>{}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" \
w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        minutes_body_xml(title, markdown)
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" \
ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" \
ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.\
document.main+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" \
Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/\
officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let doc_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let parts: [(&str, &str); 4] = [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", root_rels),
            ("word/document.xml", &document),
            ("word/_rels/document.xml.rels", doc_rels),
        ];
        for (name, content) in parts {
            writer
                .start_file(name, options)
                .map_err(|e| AppError::FileSystem {
                    message: format!("docx packaging failed for {}: {}", name, e),
                    path: name.to_string(),
                })?;
            writer
                .write_all(content.as_bytes())
                .map_err(|e| AppError::FileSystem {
                    message: format!("docx packaging failed for {}: {}", name, e),
                    path: name.to_string(),
                })?;
        }
        writer
            .finish()
            .map_err(|e| AppError::FileSystem {
                message: format!("docx packaging failed: {}", e),
                path: "minutes.docx".to_string(),
            })?;
    }
    Ok(buf.into_inner())
}

/// Write the minutes document to `save_path`.
pub fn write_minutes_docx(
    save_path: &str,
    title: &str,
    markdown: &str,
) -> Result<(), AppError> {
    let bytes = minutes_docx_bytes(title, markdown)?;
    std::fs::write(save_path, bytes).map_err(|e| AppError::FileSystem {
        message: format!("Failed to write minutes document: {}", e),
        path: save_path.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    // Feature: tauri-rewrite (meeting minutes), Property: minutes markdown
    // maps losslessly onto docx block structure
    #[test]
    fn parses_minutes_markdown_blocks() {
        let blocks = parse_blocks(
            "# 会议纪要\n\n## 概要\n讨论了发布。\n\n- **决定**：上线\n- 待办\n\n1. 张三负责回归\n2. 李四写文档\n\n```\ncode\n```\n---\n> 引用行\n",
        );
        let mut kinds = Vec::new();
        for block in &blocks {
            kinds.push(match block {
                Block::Heading(level, _) => format!("h{}", level),
                Block::Paragraph(_) => "p".to_string(),
                Block::Bullet(_) => "bullet".to_string(),
                Block::Numbered(_) => "numbered".to_string(),
            });
        }
        assert_eq!(
            kinds,
            vec!["h2", "p", "bullet", "bullet", "numbered", "numbered", "p"]
        );
        // H1 (document title) is skipped; code fence content is skipped.
        assert!(blocks.iter().all(|b| !matches!(b, Block::Heading(1, _))));
    }

    #[test]
    fn bullet_marker_is_prepended_and_bold_survives() {
        let blocks = parse_blocks("- **决定**：上线");
        match &blocks[0] {
            Block::Bullet(runs) => {
                assert_eq!(runs[0].text, "• 决定");
                assert!(runs[0].bold);
                assert_eq!(runs[1].text, "：上线");
                assert!(!runs[1].bold);
            }
            other => panic!("expected bullet, got paragraph: {}", matches!(other, Block::Paragraph(_))),
        }
    }

    #[test]
    fn inline_text_is_xml_escaped() {
        assert_eq!(xml_escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        let xml = minutes_body_xml("T&L <周会>", "讨论 <风险> 与 & 事项");
        assert!(xml.contains("&lt;风险&gt;"));
        assert!(!xml.contains("<风险>"));
        assert!(xml.contains("T&amp;L &lt;周会&gt;"));
    }

    #[test]
    fn docx_package_is_valid_zip_with_required_parts() {
        let bytes = minutes_docx_bytes("周会纪要", "## 概要\n\n- 第一项\n").unwrap();
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");
        for part in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
        ] {
            assert!(archive.by_name(part).is_ok(), "missing part {}", part);
        }
        let mut document = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document)
            .unwrap();
        assert!(document.contains("周会纪要"));
        assert!(document.contains("• 第一项"));
        // Direct formatting: title size, heading size, bold all present.
        assert!(document.contains("<w:sz w:val=\"44\"/>"));
        assert!(document.contains("<w:sz w:val=\"32\"/>"));
        assert!(document.contains("<w:b/>"));
        assert!(document.contains("<w:jc w:val=\"center\"/>"));
    }

    #[test]
    fn numbered_items_keep_their_marker() {
        let blocks = parse_blocks("3. 第3条");
        match &blocks[0] {
            Block::Numbered(runs) => assert_eq!(runs[0].text, "3. 第3条"),
            _ => panic!("expected numbered item"),
        }
        // Version-like paragraphs must not be misread as numbered items.
        let blocks = parse_blocks("版本 1.2.3 已发布");
        assert!(matches!(blocks[0], Block::Paragraph(_)));
    }
}
