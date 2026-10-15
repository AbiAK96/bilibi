//! Pure, deterministic PDF serializer for owner-selected business documents,
//! plus structural validation of any PDF bytes before they are stored.
//!
//! Layout is fixed: A4, built-in Courier 10 pt, 85 characters per line and 48
//! lines per page. Text and cells wrap without truncation; table headers repeat
//! on continuation pages. A row that fits a page stays together; larger rows
//! continue across pages. Explicit breaks never create empty trailing pages.
//! Body text is printable ASCII plus Latin-1 U+00A0–U+00FF (WinAnsi); anything
//! else is refused, never replaced. Metadata supports Unicode (UTF-16 strings).
//! The serializer adds no dates, random IDs, resources or business data, so
//! identical input yields identical bytes. It never interprets HTML or URLs.
use crate::{Error, Result};

const COLUMNS: usize = 85;
const LINES: usize = 48;
/// Bounds the memory one owner callback can demand; larger reports are split.
const MAX_PAGES: usize = 2000;
const MAX_TABLE_COLUMNS: usize = 20;

/// Optional `/Info` entries. The title is always taken from the document.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Block {
    Text(String),
    /// 1–20 columns; every row must match the header's cell count.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    PageBreak,
}
/// The owner formats dates and exact numbers into strings and chooses which of
/// its data appears; this type carries no authority and stores nothing.
#[derive(Clone, Debug)]
pub struct Document {
    pub title: String,
    pub metadata: Metadata,
    pub blocks: Vec<Block>,
}

#[derive(Default)]
struct Pages {
    done: Vec<Vec<String>>,
    current: Vec<String>,
}
impl Pages {
    fn new_page(&mut self) {
        if !self.current.is_empty() {
            self.done.push(std::mem::take(&mut self.current));
        }
    }
    fn line(&mut self, line: String) -> Result<()> {
        if self.current.len() == LINES {
            self.new_page();
        }
        if self.done.len() >= MAX_PAGES {
            return Err(Error::InvalidDocument);
        }
        self.current.push(line);
        Ok(())
    }
    fn lines(&mut self, lines: impl IntoIterator<Item = String>) -> Result<()> {
        lines.into_iter().try_for_each(|line| self.line(line))
    }
    fn ensure_space(&mut self, needed: usize) {
        if self.current.len() + needed > LINES {
            self.new_page();
        }
    }
}

fn supported(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' '..='~' | '\u{a0}'..='\u{ff}')
}
fn wrap(text: &str, width: usize) -> Result<Vec<String>> {
    if !text.chars().all(supported) {
        return Err(Error::UnsupportedText);
    }
    let text = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', " ");
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            lines.push(String::new());
        }
        lines.extend(chars.chunks(width).map(|c| c.iter().collect::<String>()));
    }
    Ok(lines)
}
fn table_row(cells: &[String], count: usize, width: usize) -> Result<Vec<String>> {
    if cells.len() != count {
        return Err(Error::InvalidDocument);
    }
    let columns = cells
        .iter()
        .map(|cell| wrap(cell, width))
        .collect::<Result<Vec<_>>>()?;
    let height = columns.iter().map(Vec::len).max().unwrap_or(1);
    Ok((0..height)
        .map(|index| {
            columns
                .iter()
                .map(|column| {
                    let cell = column.get(index).map_or("", String::as_str);
                    format!("{cell:<width$}")
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect())
}
fn table(pages: &mut Pages, headers: &[String], rows: &[Vec<String>]) -> Result<()> {
    let count = headers.len();
    if !(1..=MAX_TABLE_COLUMNS).contains(&count) {
        return Err(Error::InvalidDocument);
    }
    let width = (COLUMNS - 3 * (count - 1)) / count;
    let mut header = table_row(headers, count, width)?;
    if header.len() >= LINES {
        return Err(Error::InvalidDocument);
    }
    header.push("-".repeat(count * width + 3 * (count - 1)));
    if header.len() >= LINES {
        return Err(Error::InvalidDocument);
    }
    let first = match rows.first() {
        Some(row) => table_row(row, count, width)?
            .len()
            .min(LINES - header.len()),
        None => 0,
    };
    pages.ensure_space(header.len() + first);
    pages.lines(header.iter().cloned())?;
    let capacity = LINES - header.len();
    for row in rows {
        let lines = table_row(row, count, width)?;
        if lines.len() <= capacity && pages.current.len() + lines.len() > LINES {
            pages.new_page();
            pages.lines(header.iter().cloned())?;
        }
        for line in lines {
            if pages.current.len() == LINES {
                pages.new_page();
                pages.lines(header.iter().cloned())?;
            }
            pages.line(line)?;
        }
    }
    Ok(())
}

/// Serialize an owner-selected document. Errors never echo document contents.
pub fn render(document: &Document) -> Result<Vec<u8>> {
    let mut pages = Pages::default();
    pages.lines(wrap(&document.title, COLUMNS)?)?;
    for block in &document.blocks {
        match block {
            Block::Text(text) => pages.lines(wrap(text, COLUMNS)?)?,
            Block::PageBreak => pages.new_page(),
            Block::Table { headers, rows } => table(&mut pages, headers, rows)?,
        }
    }
    let Pages { mut done, current } = pages;
    // The title guarantees at least one line, so `current` is only empty after
    // a trailing break; an empty last page is dropped, never emitted.
    if !current.is_empty() {
        done.push(current);
    }
    Ok(serialize(&done, document))
}

/// Raw Latin-1 operand with PDF string escaping; the one seam for content, so
/// no caller can inject PDF syntax. Characters were validated by `wrap`.
fn literal(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let byte = c as u32 as u8;
        if matches!(byte, b'\\' | b'(' | b')') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out
}
fn info_string(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        out.push_str(&format!("{unit:04X}"));
    }
    out.push('>');
    out
}
fn serialize(pages: &[Vec<String>], document: &Document) -> Vec<u8> {
    let count = pages.len();
    let mut info = String::from("<<");
    let entries = [
        ("Title", Some(document.title.as_str())),
        ("Author", document.metadata.author.as_deref()),
        ("Subject", document.metadata.subject.as_deref()),
        ("Keywords", document.metadata.keywords.as_deref()),
    ];
    for (name, value) in entries {
        if let Some(value) = value {
            info.push_str(&format!(" /{name} {}", info_string(value)));
        }
    }
    info.push_str(" >>");
    let kids: String = (0..count).map(|n| format!("{} 0 R ", 5 + n * 2)).collect();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Count {count} /Kids [{kids}] >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_vec(),
        info.into_bytes(),
    ];
    for (n, page) in pages.iter().enumerate() {
        let mut stream = b"BT /F1 10 Tf 40 800 Td 15 TL\n".to_vec();
        for line in page {
            stream.push(b'(');
            stream.extend(literal(line));
            stream.extend(b") Tj T*\n");
        }
        stream.extend(b"ET\n");
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                6 + n * 2
            )
            .into_bytes(),
        );
        let mut object = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        object.extend(stream);
        object.extend(b"endstream");
        objects.push(object);
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", index + 1).into_bytes());
        out.extend(object);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    let total = objects.len() + 1;
    out.extend(format!("xref\n0 {total}\n0000000000 65535 f \n").into_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    out.extend(
        format!("trailer\n<< /Size {total} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n")
            .into_bytes(),
    );
    out
}

/// Structural check applied to every PDF before storage, whoever produced it:
/// header, `%%EOF` trailer, and a `startxref` offset landing on a classic xref
/// table or an indirect object (xref-stream files). It is not a complete PDF
/// validator: the producing renderer remains responsible for its content.
pub fn validate(bytes: &[u8]) -> Result<()> {
    let tail = &bytes[bytes.len().saturating_sub(1024)..];
    let eof = rfind(tail, b"%%EOF").ok_or(Error::InvalidPdf)?;
    if !tail[eof + 5..].iter().all(u8::is_ascii_whitespace) {
        return Err(Error::InvalidPdf);
    }
    let start = rfind(tail, b"startxref").ok_or(Error::InvalidPdf)?;
    let digits: String = tail
        .get(start + 9..eof)
        .ok_or(Error::InvalidPdf)?
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|b| *b as char)
        .collect();
    let offset: usize = digits.parse().map_err(|_| Error::InvalidPdf)?;
    let target = bytes.get(offset..).ok_or(Error::InvalidPdf)?;
    let header = bytes.starts_with(b"%PDF-") && bytes.len() > 16;
    if header && (target.starts_with(b"xref") || indirect_object(target)) {
        Ok(())
    } else {
        Err(Error::InvalidPdf)
    }
}
fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}
fn indirect_object(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(32)];
    let mut parts = head.split(|b| *b == b' ');
    let number = |part: Option<&[u8]>| {
        part.is_some_and(|p| !p.is_empty() && p.iter().all(u8::is_ascii_digit))
    };
    number(parts.next())
        && number(parts.next())
        && parts.next().is_some_and(|p| p.starts_with(b"obj"))
}
