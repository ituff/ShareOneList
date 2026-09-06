// Plain-text extraction from Office (OOXML) and PDF files, used for AI
// context injection. OOXML formats are zip containers with XML parts; text
// lives in well-known elements (w:t, a:t, si/t). PDFs use pdf-extract.

use crate::errors::AppError;

/// Extracts plain text from a file's bytes, routing by file extension.
/// Unknown extensions are returned as lossy UTF-8 (the plain-text path).
pub fn extract_from_bytes(file_name: &str, bytes: &[u8]) -> Result<String, AppError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" => docx_text(bytes),
        "pptx" => pptx_text(bytes),
        "xlsx" => xlsx_text(bytes),
        "pdf" => pdf_text(bytes),
        _ => Ok(String::from_utf8_lossy(bytes).to_string()),
    }
}

/// Local (namespace-stripped) name of an XML tag like `w:document`.
fn local_name(tag: &[u8]) -> &[u8] {
    match tag.iter().rposition(|&b| b == b':') {
        Some(pos) => &tag[pos + 1..],
        None => tag,
    }
}

/// Collects paragraph text from an OOXML XML part: text accumulates inside
/// `text_tag` elements; each `para_tag` element yields one paragraph.
fn paragraphs_from_xml(xml: &str, text_tag: &str, para_tag: &str) -> Vec<String> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().0);
                if name == para_tag.as_bytes() {
                    current.clear();
                } else if name == text_tag.as_bytes() {
                    in_text = true;
                }
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    current.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().0);
                if name == text_tag.as_bytes() {
                    in_text = false;
                } else if name == para_tag.as_bytes() && !current.trim().is_empty() {
                    paragraphs.push(current.trim().to_string());
                    current.clear();
                }
            }
            Ok(Event::Eof) => break,
            // Tolerate malformed parts: keep what we have so far.
            Err(_) => break,
            _ => {}
        }
    }
    if !current.trim().is_empty() {
        paragraphs.push(current.trim().to_string());
    }
    paragraphs
}

/// Unzips an OOXML container and returns the named part(s); entries whose
/// name matches `filter` are returned in zip order.
fn read_parts(bytes: &[u8], filter: impl Fn(&str) -> bool) -> Result<Vec<Vec<u8>>, AppError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        AppError::Validation {
            message: format!("not a readable Office document: {}", e),
            field: "item_id".to_string(),
        }
    })?;
    let mut parts = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| AppError::Validation {
            message: format!("corrupt Office document: {}", e),
            field: "item_id".to_string(),
        })?;
        let name = entry.name().to_string();
        if filter(&name) {
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| AppError::Validation {
                message: format!("corrupt Office document: {}", e),
                field: "item_id".to_string(),
            })?;
            parts.push(data);
        }
    }
    Ok(parts)
}

/// Word document: paragraphs from `word/document.xml`.
pub fn docx_text(bytes: &[u8]) -> Result<String, AppError> {
    let parts = read_parts(bytes, |name| name == "word/document.xml")?;
    let Some(part) = parts.first() else {
        return Err(AppError::Validation {
            message: "document.xml missing from docx container".into(),
            field: "item_id".to_string(),
        });
    };
    let xml = String::from_utf8_lossy(part);
    Ok(paragraphs_from_xml(&xml, "t", "p").join("\n"))
}

/// PowerPoint deck: slide texts from `ppt/slides/slideN.xml`, in slide order.
pub fn pptx_text(bytes: &[u8]) -> Result<String, AppError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        AppError::Validation {
            message: format!("not a readable Office document: {}", e),
            field: "item_id".to_string(),
        }
    })?;
    let mut slides: Vec<(u32, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let name = archive
            .by_index_raw(i)
            .map_err(|e| AppError::Validation {
                message: format!("corrupt Office document: {}", e),
                field: "item_id".to_string(),
            })?
            .name()
            .to_string();
        let Some(number) = name
            .strip_prefix("ppt/slides/slide")
            .and_then(|rest| rest.strip_suffix(".xml"))
            .and_then(|rest| rest.parse::<u32>().ok())
        else {
            continue;
        };
        let mut entry = archive.by_name(&name).map_err(|e| AppError::Validation {
            message: format!("corrupt Office document: {}", e),
            field: "item_id".to_string(),
        })?;
        let mut data = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| AppError::Validation {
            message: format!("corrupt Office document: {}", e),
            field: "item_id".to_string(),
        })?;
        slides.push((number, data));
    }
    if slides.is_empty() {
        return Err(AppError::Validation {
            message: "no slides found in pptx container".into(),
            field: "item_id".to_string(),
        });
    }
    slides.sort_by_key(|(n, _)| *n);
    let mut pages = Vec::new();
    for (_, part) in slides {
        let xml = String::from_utf8_lossy(&part);
        let slide = paragraphs_from_xml(&xml, "t", "p").join("\n");
        if !slide.is_empty() {
            pages.push(slide);
        }
    }
    Ok(pages.join("\n\n"))
}

/// Excel workbook: shared string table entries (text cells). Numbers and
/// formulas are skipped — for AI context the text cells carry the content.
pub fn xlsx_text(bytes: &[u8]) -> Result<String, AppError> {
    let parts = read_parts(bytes, |name| name == "xl/sharedStrings.xml")?;
    let Some(part) = parts.first() else {
        return Err(AppError::Validation {
            message: "sharedStrings.xml missing from xlsx container".into(),
            field: "item_id".to_string(),
        });
    };
    let xml = String::from_utf8_lossy(part);
    Ok(paragraphs_from_xml(&xml, "t", "si").join("\n"))
}

/// PDF text via pdf-extract.
pub fn pdf_text(bytes: &[u8]) -> Result<String, AppError> {
    pdf_extract::extract_text_from_mem(bytes).map_err(|e| AppError::Validation {
        message: format!("failed to extract PDF text: {}", e),
        field: "item_id".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds an in-memory zip with the given (name, content) entries.
    fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options: zip::write::SimpleFileOptions = Default::default();
            for (name, content) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn docx_extracts_paragraphs_in_order() {
        let bytes = make_zip(&[(
            "word/document.xml",
            "<?xml version=\"1.0\"?><w:document xmlns:w=\"x\"><w:body>\
             <w:p><w:r><w:t>第一段，含 &amp; 符号</w:t></w:r></w:p>\
             <w:p><w:r><w:t xml:space=\"preserve\">Second  paragraph</w:t></w:r></w:p>\
             <w:p><w:r><w:t></w:t></w:r></w:p>\
             </w:body></w:document>",
        )]);
        let text = docx_text(&bytes).unwrap();
        assert_eq!(text, "第一段，含 & 符号\nSecond  paragraph");
    }

    #[test]
    fn pptx_orders_slides_by_number() {
        let bytes = make_zip(&[
            (
                "ppt/slides/slide2.xml",
                "<p:sld xmlns:p=\"x\"><p:cSld><p:txBody><a:p xmlns:a=\"y\"><a:r><a:t>Slide two</a:t></a:r></a:p></p:txBody></p:cSld></p:sld>",
            ),
            (
                "ppt/slides/slide1.xml",
                "<p:sld xmlns:p=\"x\"><p:cSld><p:txBody><a:p xmlns:a=\"y\"><a:r><a:t>Slide one</a:t></a:r></a:p><a:p xmlns:a=\"y\"><a:r><a:t>second para</a:t></a:r></a:p></p:txBody></p:cSld></p:sld>",
            ),
            ("ppt/media/image1.png", "not xml"),
        ]);
        let text = pptx_text(&bytes).unwrap();
        assert_eq!(text, "Slide one\nsecond para\n\nSlide two");
    }

    #[test]
    fn xlsx_reads_shared_strings() {
        let bytes = make_zip(&[(
            "xl/sharedStrings.xml",
            "<?xml version=\"1.0\"?><sst xmlns=\"x\">\
             <si><t>产品名称</t></si>\
             <si><t>混合文本 <r><t>续</t></r></t></si>\
             <si><t>第三项</t></si>\
             </sst>",
        )]);
        let text = xlsx_text(&bytes).unwrap();
        assert_eq!(text, "产品名称\n混合文本 续\n第三项");
    }

    #[test]
    fn extract_routes_by_extension() {
        let bytes = make_zip(&[(
            "word/document.xml",
            "<w:document xmlns:w=\"x\"><w:p><w:t>hello doc</w:t></w:p></w:document>",
        )]);
        assert_eq!(extract_from_bytes("report.DOCX", &bytes).unwrap(), "hello doc");
        // Unknown extension falls back to lossy UTF-8.
        assert_eq!(extract_from_bytes("notes.txt", "plain".as_bytes()).unwrap(), "plain");
    }

    #[test]
    fn docx_rejects_non_zip() {
        assert!(docx_text(b"not a zip").is_err());
    }

    // Minimal but structurally valid single-page PDF with a text object.
    #[test]
    fn pdf_extracts_simple_text() {
        let mut objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            String::new(), // stream object filled below
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let stream = b"BT /F1 12 Tf 72 720 Td (Hello from PDF) Tj ET";
        objects[3] = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            std::str::from_utf8(stream).unwrap()
        );

        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, body).as_bytes());
        }
        let xref_pos = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            pdf.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
                objects.len() + 1,
                xref_pos
            )
            .as_bytes(),
        );

        let text = pdf_text(&pdf).unwrap();
        assert!(
            text.contains("Hello from PDF"),
            "unexpected extracted text: {:?}",
            text
        );
    }
}
