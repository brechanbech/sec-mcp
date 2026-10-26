//! Filing text: turning an EDGAR document into text a model can page through.
//!
//! Everything here is pure, so it is tested without the network; fetching and
//! caching live with the client in `main.rs`. The design, and the prototype
//! measurements behind it, are in `docs/filing-text-design.md`.
//!
//! EDGAR documents come in two kinds of HTML. Modern 10-Ks and 10-Qs are inline
//! XBRL: well-formed XHTML padded with hidden XBRL contexts. Exhibits prepared
//! by filing agents are legacy HTML wrapped in an SGML envelope, and are not
//! XML at all (unclosed `<img>`, unclosed `<TYPE>`). html5ever's tokenizer reads
//! both. Text extraction needs only the stream of tags and text in order, not a
//! corrected tree, so the tree builder is not used.

use anyhow::{bail, Result};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use std::cell::RefCell;

/// A document reduced to text, with what a reader needs to navigate it.
pub struct Document {
    pub text: String,
    /// Images in the document. Their text, if any, is not in `text`.
    pub images: usize,
    pub sections: Vec<Section>,
}

impl Document {
    pub fn from_html(html: &str) -> Self {
        let (text, images) = html_to_text(html);
        let sections = sections(&text);
        Self {
            text,
            images,
            sections,
        }
    }

    /// Older filings are plain text inside the same SGML envelope.
    pub fn from_plain(raw: &str) -> Self {
        let mut text = String::with_capacity(raw.len());
        for line in raw.lines() {
            let trimmed = line.trim_end();
            if is_sgml_envelope_line(trimmed) {
                continue;
            }
            text.push_str(trimmed);
            text.push('\n');
        }
        let sections = sections(&text);
        Self {
            text,
            images: 0,
            sections,
        }
    }
}

fn is_sgml_envelope_line(line: &str) -> bool {
    const ENVELOPE: &[&str] = &[
        "<DOCUMENT>",
        "</DOCUMENT>",
        "<TEXT>",
        "</TEXT>",
        "<TYPE>",
        "<SEQUENCE>",
        "<FILENAME>",
        "<DESCRIPTION>",
        "<PDF>",
        "</PDF>",
    ];
    let upper = line.trim_start().to_ascii_uppercase();
    ENVELOPE.iter().any(|tag| upper.starts_with(tag))
}

// ── HTML to text ──────────────────────────────────────────────────────────────

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "wbr",
];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "tr",
    "ul",
    "ol",
    "hr",
    "center",
    "pre",
    "blockquote",
    "dt",
    "dd",
];
/// Never shown: document metadata, and inline XBRL's hidden contexts.
const HIDDEN: &[&str] = &["head", "script", "style", "title", "ix:header"];
/// The SGML envelope around an agent-prepared document. These tags are never
/// closed, and each value runs to the next tag, so they must not open an
/// element: treated as one, `<TYPE>` hid the whole of the first exhibit tried.
const ENVELOPE: &[&str] = &["type", "sequence", "filename", "description"];

#[derive(Default)]
struct Walker {
    /// Open elements, and whether each hides its content.
    stack: Vec<(String, bool)>,
    out: String,
    line: String,
    row: Option<Vec<String>>,
    cell: Option<String>,
    images: usize,
    skip_text: bool,
}

impl Walker {
    fn hidden(&self) -> bool {
        self.stack.iter().any(|(_, hidden)| *hidden)
    }

    fn push_text(&mut self, s: &str) {
        let target = match self.cell.as_mut() {
            Some(cell) => cell,
            None => &mut self.line,
        };
        for ch in s.chars() {
            let ch = if ch.is_whitespace() { ' ' } else { ch };
            if ch == ' ' && (target.is_empty() || target.ends_with(' ')) {
                continue;
            }
            target.push(ch);
        }
    }

    fn end_line(&mut self) {
        let line = self.line.trim();
        if !line.is_empty() {
            self.out.push_str(line);
            self.out.push('\n');
        }
        self.line.clear();
    }

    fn end_cell(&mut self) {
        if let (Some(cell), Some(row)) = (self.cell.take(), self.row.as_mut()) {
            row.push(cell.trim().to_string());
        }
    }

    /// One table row becomes one line, cells joined by ` | `.
    ///
    /// EDGAR tables put `$`, `1,234` and `)` or `%` in separate cells, so a
    /// literal rendering reads `$ | 1,234 | )`. Those fragments are joined back
    /// onto their number, and empty spacer cells are dropped.
    fn end_row(&mut self) {
        self.end_cell();
        let Some(cells) = self.row.take() else {
            return;
        };
        let mut merged: Vec<String> = Vec::new();
        let mut prefix = String::new();
        for cell in cells.into_iter().filter(|c| !c.is_empty()) {
            if matches!(cell.as_str(), "$" | "(" | "($" | "$(") {
                prefix.push_str(&cell);
                continue;
            }
            if matches!(cell.as_str(), ")" | "%" | ")%" | "%)") {
                if let Some(last) = merged.last_mut() {
                    last.push_str(&cell);
                    continue;
                }
            }
            merged.push(format!("{prefix}{cell}"));
            prefix.clear();
        }
        if !merged.is_empty() {
            self.end_line();
            self.out.push_str(&merged.join(" | "));
            self.out.push('\n');
        }
    }

    fn start_tag(&mut self, name: &str, hides: bool, opens: bool) {
        if !self.hidden() {
            match name {
                "img" => self.images += 1,
                "tr" => {
                    self.end_row();
                    self.row = Some(Vec::new());
                }
                "td" | "th" => {
                    self.end_cell();
                    if self.row.is_some() {
                        self.cell = Some(String::new());
                    }
                }
                _ if BLOCK.contains(&name) && self.cell.is_none() => self.end_line(),
                _ => {}
            }
        }
        if opens {
            self.stack.push((name.to_string(), hides));
        }
    }

    fn end_tag(&mut self, name: &str) {
        // Close back to the matching element; a stray end tag closes nothing.
        if let Some(pos) = self.stack.iter().rposition(|(open, _)| open == name) {
            self.stack.truncate(pos);
        }
        if !self.hidden() {
            match name {
                "tr" | "table" => self.end_row(),
                "td" | "th" => self.end_cell(),
                _ if BLOCK.contains(&name) && self.cell.is_none() => self.end_line(),
                _ => {}
            }
        }
    }
}

struct Sink(RefCell<Walker>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut walker = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_ascii_lowercase();
                match tag.kind {
                    TagKind::StartTag if ENVELOPE.contains(&name.as_str()) => {
                        walker.skip_text = true;
                    }
                    TagKind::StartTag => {
                        walker.skip_text = false;
                        let hides = HIDDEN.contains(&name.as_str())
                            || tag.attrs.iter().any(|a| {
                                &*a.name.local == "style"
                                    && a.value
                                        .to_ascii_lowercase()
                                        .replace(' ', "")
                                        .contains("display:none")
                            });
                        let opens = !VOID.contains(&name.as_str()) && !tag.self_closing;
                        walker.start_tag(&name, hides, opens);
                    }
                    TagKind::EndTag => {
                        walker.skip_text = false;
                        walker.end_tag(&name);
                    }
                }
            }
            Token::CharacterTokens(text) => {
                if walker.skip_text {
                    walker.skip_text = false;
                } else if !walker.hidden() {
                    walker.push_text(&text);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Visible text, one line per block or table row, and the image count.
fn html_to_text(html: &str) -> (String, usize) {
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(Walker::default())),
        TokenizerOpts::default(),
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(html));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut walker = tokenizer.sink.0.borrow_mut();
    walker.end_row();
    walker.end_line();
    (std::mem::take(&mut walker.out), walker.images)
}

// ── Sections ──────────────────────────────────────────────────────────────────

/// An "Item 1A. Risk Factors" heading and where it starts in the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// `Part II, Item 1A`, or `Item 2.02` where the form has no parts.
    pub label: String,
    pub part: Option<String>,
    pub item: String,
    pub title: String,
    pub offset: usize,
}

/// Item headings, in order. Best effort, and labelled as such to callers.
///
/// The part is tracked because 10-Q item numbers restart in Part II. A heading
/// may be a line or a two-cell row (`Item 1A. | Risk Factors`), which is how
/// older and agent-prepared filings lay them out. Table-of-contents rows end in
/// a page-number cell and are skipped; a heading followed almost at once by the
/// next is a contents listing too.
pub fn sections(text: &str) -> Vec<Section> {
    let mut found: Vec<Section> = Vec::new();
    let mut part: Option<String> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let start = pos;
        pos += line.len();
        let line = line.trim();
        if line.len() >= 160 {
            continue;
        }
        let cells: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let contents_row = cells.len() > 1
            && cells
                .last()
                .is_some_and(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_digit()));
        if contents_row {
            continue;
        }
        let first = cells[0];
        let lower = first.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("part ") {
            let numeral: String = rest
                .chars()
                .take_while(|c| matches!(c, 'i' | 'v' | 'x'))
                .collect();
            let ends_word = rest[numeral.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if !numeral.is_empty() && ends_word {
                part = Some(numeral.to_ascii_uppercase());
            }
            continue;
        }
        let Some(rest) = lower.strip_prefix("item") else {
            continue;
        };
        let rest = rest.trim_start();
        let item: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let item = item.trim_end_matches('.').to_ascii_uppercase();
        if item.is_empty() || item.len() > 5 || !item.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        // The title keeps the filing's own case, so take it from `first`, not
        // the lower-cased copy.
        let title = if cells.len() > 1 {
            cells[1].to_string()
        } else {
            let after = first.len() - rest.len() + item.len();
            first
                .get(after..)
                .unwrap_or("")
                .trim_start_matches(['.', ':', ' '])
                .trim()
                .to_string()
        };
        let label = match &part {
            Some(p) => format!("Part {p}, Item {item}"),
            None => format!("Item {item}"),
        };
        found.push(Section {
            label,
            part: part.clone(),
            item,
            title,
            offset: start,
        });
    }
    let mut kept = Vec::with_capacity(found.len());
    for (i, section) in found.iter().enumerate() {
        let next = found.get(i + 1).map_or(text.len(), |s| s.offset);
        if next - section.offset > 150 {
            kept.push(section.clone());
        }
    }
    kept
}

/// Find the section a caller named: `1A`, `Item 1A`, `Part II, Item 1A`, `2.02`.
///
/// Without a part, the first matching item wins; 10-Q Part I and Part II both
/// have an Item 1, and the caller is told when another exists.
pub fn find_section<'a>(sections: &'a [Section], wanted: &str) -> Option<&'a Section> {
    let lower = wanted.trim().to_ascii_lowercase();
    let (part, item) = match lower.split_once("item") {
        Some((before, after)) => {
            let part = before
                .trim()
                .trim_end_matches(',')
                .trim()
                .strip_prefix("part")
                .map(|p| p.trim().to_ascii_uppercase());
            (part, after.trim())
        }
        None => (None, lower.as_str()),
    };
    let item = item.trim_end_matches('.').to_ascii_uppercase();
    sections.iter().find(|s| {
        s.item == item
            && part
                .as_ref()
                .is_none_or(|p| s.part.as_deref() == Some(p.as_str()))
    })
}

// ── Paging and search ─────────────────────────────────────────────────────────

/// Where a page that starts at `start` should end: at most `limit` bytes on,
/// never past `end`, and on a line boundary when one is in reach, so a page
/// never stops mid-row.
pub fn page_end(text: &str, start: usize, end: usize, limit: usize) -> usize {
    let hard = (start + limit).min(end).min(text.len());
    if hard >= end {
        return end;
    }
    let mut cut = hard;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    match text[start..cut].rfind('\n') {
        Some(nl) if nl > 0 => start + nl + 1,
        _ => cut,
    }
}

/// Snap a caller-supplied position back to a character boundary.
pub fn floor_boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

pub struct Match {
    pub offset: usize,
    pub snippet: String,
}

/// Case-insensitive search from `from`: the total number of matches, and the
/// first `max` with about 400 characters of context each.
///
/// ASCII lower-casing keeps every byte where it was, so offsets found in the
/// lowered copy are offsets in the text.
pub fn find(text: &str, needle: &str, from: usize, max: usize) -> (usize, Vec<Match>) {
    let needle = needle.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return (0, Vec::new());
    }
    let haystack = text.to_ascii_lowercase();
    let from = floor_boundary(text, from);
    let mut total = 0;
    let mut matches = Vec::new();
    for (at, _) in haystack[from..].match_indices(&needle) {
        let offset = from + at;
        total += 1;
        if matches.len() < max {
            let lo = floor_boundary(text, offset.saturating_sub(200));
            let hi = floor_boundary(text, offset + needle.len() + 200);
            matches.push(Match {
                offset,
                snippet: text[lo..hi].to_string(),
            });
        }
    }
    (total, matches)
}

// ── Locating a filing ─────────────────────────────────────────────────────────

/// A filing, or one document in it, in EDGAR's Archives.
#[derive(Debug, PartialEq)]
pub struct ArchivesRef {
    /// Unpadded, as Archives paths take it.
    pub cik: String,
    /// Dashed: `0001193125-25-109934`.
    pub accession: String,
    pub document: Option<String>,
}

impl ArchivesRef {
    pub fn new(cik: &str, accession: &str, document: Option<&str>) -> Result<Self> {
        let cik = cik.trim_start_matches('0');
        if cik.is_empty() || cik.len() > 10 || !cik.bytes().all(|b| b.is_ascii_digit()) {
            bail!("'{cik}' is not a CIK");
        }
        let Some(accession) = normalize_accession(accession) else {
            bail!("'{accession}' is not an accession number (expected 0001193125-25-109934)");
        };
        let document = match document.map(str::trim).filter(|d| !d.is_empty()) {
            Some(name) if is_index_file(name) => None,
            Some(name) => Some(check_document_name(name)?.to_string()),
            None => None,
        };
        Ok(Self {
            cik: cik.to_string(),
            accession,
            document,
        })
    }

    fn folder(&self) -> String {
        format!(
            "https://www.sec.gov/Archives/edgar/data/{}/{}",
            self.cik,
            self.accession.replace('-', "")
        )
    }

    pub fn headers_url(&self) -> String {
        format!("{}/{}-index-headers.html", self.folder(), self.accession)
    }

    pub fn document_url(&self, name: &str) -> String {
        format!("{}/{name}", self.folder())
    }
}

/// `0001193125-25-109934` or `000119312525109934`, returned dashed.
pub fn normalize_accession(s: &str) -> Option<String> {
    let digits: String = s.trim().chars().filter(|c| *c != '-').collect();
    if digits.len() != 18 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &digits[..10],
        &digits[10..12],
        &digits[12..]
    ))
}

/// Document names are a single path segment of plain characters, so a name
/// can never climb out of its filing's folder.
fn check_document_name(name: &str) -> Result<&str> {
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if !plain || name.starts_with('.') || name.len() > 120 {
        bail!("'{name}' is not a document name in an EDGAR filing");
    }
    Ok(name)
}

/// EDGAR's own listing pages for a filing, which name the filing, not a document.
fn is_index_file(name: &str) -> bool {
    name.ends_with("-index.htm")
        || name.ends_with("-index.html")
        || name.ends_with("-index-headers.html")
        || name == "index.json"
        || name == "index.html"
}

/// Parse a URL into a filing reference, accepting only EDGAR's Archives.
///
/// The URL is model-controlled, and this tool must not become a general
/// fetcher, so anything but `https://www.sec.gov/Archives/edgar/data/…` is
/// refused. The inline-XBRL viewer form (`/ix?doc=/Archives/…`), which EDGAR's
/// own pages link to, is unwrapped.
pub fn parse_archives_url(url: &str) -> Result<ArchivesRef> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://www.sec.gov")
        .or_else(|| url.strip_prefix("https://sec.gov"));
    let Some(rest) = rest else {
        bail!("only https://www.sec.gov/Archives/edgar/data/… URLs can be read; got '{url}'");
    };
    let rest = rest.strip_prefix("/ix?doc=").unwrap_or(rest);
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let Some(path) = path.strip_prefix("/Archives/edgar/data/") else {
        bail!("only https://www.sec.gov/Archives/edgar/data/… URLs can be read; got '{url}'");
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [cik, accession] => ArchivesRef::new(cik, accession, None),
        [cik, accession, document] => ArchivesRef::new(cik, accession, Some(document)),
        _ => bail!("'{url}' does not name an EDGAR filing or a document in one"),
    }
}

/// What a document's name says about how to read it.
pub enum DocKind {
    Html,
    Plain,
    Unsupported(&'static str),
}

pub fn document_kind(name: &str) -> DocKind {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext {
        "htm" | "html" | "xhtml" => DocKind::Html,
        "txt" => DocKind::Plain,
        "pdf" => DocKind::Unsupported(
            "PDFs are not read; filings carry an HTML or text version of the same document",
        ),
        "jpg" | "jpeg" | "gif" | "png" => DocKind::Unsupported("images carry no extractable text"),
        "xml" => DocKind::Unsupported(
            "XML is structured data, not prose: use sec_insider_transactions for Forms 3, 4 \
             and 5, sec_beneficial_ownership for Schedules 13D and 13G, and the XBRL tools \
             for financial data",
        ),
        "xsd" | "zip" | "json" | "xlsx" | "css" | "js" => {
            DocKind::Unsupported("this is a supporting file, not a document to read")
        }
        _ => DocKind::Unsupported("not a document type this tool can read"),
    }
}

// ── The filing header ─────────────────────────────────────────────────────────

/// What `{accession}-index-headers.html` says about a filing.
#[derive(Debug, Default)]
pub struct FilingHeader {
    pub form: Option<String>,
    pub filed: Option<String>,
    pub period: Option<String>,
    pub company: Option<String>,
    pub documents: Vec<DocEntry>,
}

#[derive(Debug, Default, Clone)]
pub struct DocEntry {
    pub doc_type: String,
    pub sequence: Option<String>,
    pub filename: String,
    pub description: Option<String>,
}

impl DocEntry {
    /// XBRL schemas and linkbases, graphics and archives: part of the filing,
    /// but nothing a reader opens. Left out of listings, and counted.
    pub fn is_supporting(&self) -> bool {
        let t = self.doc_type.to_ascii_uppercase();
        t.starts_with("EX-101")
            || matches!(t.as_str(), "GRAPHIC" | "ZIP" | "XML" | "JSON" | "EXCEL")
    }
}

/// Read the filing's SGML header, the cheapest complete description EDGAR
/// offers: about 2 KB with every document's type, where `index.json` has
/// names and sizes only.
///
/// The page carries the header twice, once raw inside a comment and once
/// entity-escaped in a `<PRE>`; decoding the entities makes both read alike.
/// Header fields are taken at their first occurrence outside any `<DOCUMENT>`,
/// so a filing with several filers reports the first.
pub fn parse_index_headers(page: &str) -> FilingHeader {
    let decoded = page
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let mut header = FilingHeader::default();
    let mut current: Option<DocEntry> = None;
    for line in decoded.lines() {
        // EDGAR puts each marker on its own line today; tolerate the `<PRE>`
        // wrapper sharing one rather than lose the first or last document.
        let line = line
            .trim()
            .trim_start_matches("<PRE>")
            .trim_end_matches("</PRE>")
            .trim();
        if line.contains("<DOCUMENT>") {
            current = Some(DocEntry::default());
            continue;
        }
        if line.contains("</DOCUMENT>") {
            if let Some(doc) = current.take().filter(|d| !d.filename.is_empty()) {
                header.documents.push(doc);
            }
            continue;
        }
        let Some((tag, value)) = line
            .strip_prefix('<')
            .and_then(|l| l.split_once('>'))
            .map(|(t, v)| (t, v.trim()))
        else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        match current.as_mut() {
            Some(doc) => match tag {
                "TYPE" => doc.doc_type = value.to_string(),
                "SEQUENCE" => doc.sequence = Some(value.to_string()),
                "FILENAME" => doc.filename = value.to_string(),
                "DESCRIPTION" => doc.description = Some(value.to_string()),
                _ => {}
            },
            None => {
                let slot = match tag {
                    "TYPE" => &mut header.form,
                    "FILING-DATE" => &mut header.filed,
                    "PERIOD" => &mut header.period,
                    "CONFORMED-NAME" => &mut header.company,
                    _ => continue,
                };
                if slot.is_none() {
                    *slot = Some(match tag {
                        "FILING-DATE" | "PERIOD" => iso_date(value),
                        _ => value.to_string(),
                    });
                }
            }
        }
    }
    header
}

/// `20250501` as `2025-05-01`; anything else unchanged.
fn iso_date(yyyymmdd: &str) -> String {
    if yyyymmdd.len() == 8 && yyyymmdd.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &yyyymmdd[..4], &yyyymmdd[4..6], &yyyymmdd[6..])
    } else {
        yyyymmdd.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_content_is_dropped() {
        let html = r#"<html><head><title>T</title></head><body>
            <div style="display: none"><ix:header>context junk</ix:header></div>
            <p>Visible one.</p><p style="DISPLAY:NONE">hidden</p><p>Visible two.</p>
            </body></html>"#;
        let doc = Document::from_html(html);
        assert_eq!(doc.text, "Visible one.\nVisible two.\n");
    }

    #[test]
    fn the_sgml_envelope_does_not_hide_the_document() {
        // Agent-prepared exhibits open like this, with tags that never close.
        let html = "<DOCUMENT>\n<TYPE>EX-99.1\n<SEQUENCE>2\n<FILENAME>d1dex991.htm\n\
                    <DESCRIPTION>EX-99.1\n<TEXT>\n<HTML><BODY><p align=center><img src=a.jpg></p>\
                    <p>Q2 revenue of $2.99 billion.</p></BODY></HTML>\n</TEXT>\n</DOCUMENT>";
        let doc = Document::from_html(html);
        assert_eq!(doc.text, "Q2 revenue of $2.99 billion.\n");
        assert_eq!(doc.images, 1);
    }

    #[test]
    fn table_rows_rejoin_split_dollar_and_percent_cells() {
        let html = "<table><tr><td>Revenue</td><td>$</td><td>1,234</td><td></td>\
                    <td>(</td><td>56</td><td>)</td><td>12</td><td>%</td></tr></table>";
        assert_eq!(
            Document::from_html(html).text,
            "Revenue | $1,234 | (56) | 12%\n"
        );
    }

    #[test]
    fn sections_track_parts_and_skip_the_contents() {
        let filler = "x".repeat(200);
        let text = format!(
            "Item 1. | Business | 4\nItem 1A. | Risk Factors | 12\n\
             PART I\nItem 1. Business\n{filler}\nItem 1A. | Risk Factors\n{filler}\n\
             PART II\nItem 1. Legal Proceedings\n{filler}\n"
        );
        let found = sections(&text);
        let labels: Vec<&str> = found.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Part I, Item 1", "Part I, Item 1A", "Part II, Item 1"]
        );
        assert_eq!(found[1].title, "Risk Factors");
        assert_eq!(found[0].title, "Business");
        assert_eq!(&text[found[2].offset..found[2].offset + 7], "Item 1.");

        assert_eq!(find_section(&found, "1a").unwrap().label, "Part I, Item 1A");
        assert_eq!(
            find_section(&found, "Item 1").unwrap().label,
            "Part I, Item 1"
        );
        assert_eq!(
            find_section(&found, "Part II, Item 1").unwrap().title,
            "Legal Proceedings"
        );
        assert!(find_section(&found, "7").is_none());
    }

    #[test]
    fn eight_k_items_are_decimal() {
        let filler = "y".repeat(200);
        let text = format!(
            "Item 2.02. | Results of Operations and Financial Condition.\n{filler}\n\
             Item 9.01 Financial Statements and Exhibits.\n{filler}\n"
        );
        let found = sections(&text);
        assert_eq!(found[0].label, "Item 2.02");
        assert_eq!(found[1].title, "Financial Statements and Exhibits.");
        assert_eq!(find_section(&found, "2.02").unwrap().item, "2.02");
    }

    #[test]
    fn pages_end_on_a_line_and_find_reports_offsets() {
        let text = "alpha line\nbeta line\ngamma line\n";
        assert_eq!(page_end(text, 0, text.len(), 15), 11);
        assert_eq!(page_end(text, 11, text.len(), 100), text.len());
        let (total, matches) = find(text, "LINE", 0, 2);
        assert_eq!(total, 3);
        assert_eq!(matches.len(), 2);
        assert_eq!(&text[matches[1].offset..matches[1].offset + 4], "line");
        let (later, _) = find(text, "line", 12, 10);
        assert_eq!(later, 2);
    }

    #[test]
    fn only_archives_urls_are_accepted() {
        let r = parse_archives_url(
            "https://www.sec.gov/Archives/edgar/data/1559720/000119312525109934/d40594dex991.htm",
        )
        .unwrap();
        assert_eq!(r.cik, "1559720");
        assert_eq!(r.accession, "0001193125-25-109934");
        assert_eq!(r.document.as_deref(), Some("d40594dex991.htm"));
        assert_eq!(
            r.headers_url(),
            "https://www.sec.gov/Archives/edgar/data/1559720/000119312525109934/\
             0001193125-25-109934-index-headers.html"
        );

        let viewer = parse_archives_url(
            "https://www.sec.gov/ix?doc=/Archives/edgar/data/1559720/000155972025000010/abnb-20241231.htm",
        )
        .unwrap();
        assert_eq!(viewer.document.as_deref(), Some("abnb-20241231.htm"));

        let index = parse_archives_url(
            "https://www.sec.gov/Archives/edgar/data/1559720/000119312525109934/0001193125-25-109934-index.htm",
        )
        .unwrap();
        assert_eq!(index.document, None);

        for bad in [
            "https://example.com/Archives/edgar/data/1/000119312525109934/a.htm",
            "http://www.sec.gov/Archives/edgar/data/1/000119312525109934/a.htm",
            "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany",
            "https://www.sec.gov/Archives/edgar/data/1/000119312525109934/../x.htm",
            "https://www.sec.gov/Archives/edgar/data/1/12345/a.htm",
            "https://www.sec.gov.evil.com/Archives/edgar/data/1/000119312525109934/a.htm",
        ] {
            assert!(parse_archives_url(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn the_index_header_lists_documents_with_their_types() {
        let page = "<HTML><!--\n<TYPE>8-K\n<PERIOD>20250501\n<FILING-DATE>20250501\n\
                    <CONFORMED-NAME>Airbnb, Inc.\n-->\n<PRE>&lt;DOCUMENT&gt;\n&lt;TYPE&gt;8-K\n\
                    &lt;SEQUENCE&gt;1\n&lt;FILENAME&gt;d40594d8k.htm\n&lt;DESCRIPTION&gt;8-K\n\
                    &lt;/DOCUMENT&gt;\n&lt;DOCUMENT&gt;\n&lt;TYPE&gt;EX-99.1\n&lt;SEQUENCE&gt;2\n\
                    &lt;FILENAME&gt;d40594dex991.htm\n&lt;/DOCUMENT&gt;\n&lt;DOCUMENT&gt;\n\
                    &lt;TYPE&gt;EX-101.SCH\n&lt;FILENAME&gt;abnb.xsd\n&lt;/DOCUMENT&gt;</PRE>";
        let header = parse_index_headers(page);
        assert_eq!(header.form.as_deref(), Some("8-K"));
        assert_eq!(header.filed.as_deref(), Some("2025-05-01"));
        assert_eq!(header.company.as_deref(), Some("Airbnb, Inc."));
        let types: Vec<&str> = header
            .documents
            .iter()
            .map(|d| d.doc_type.as_str())
            .collect();
        assert_eq!(types, ["8-K", "EX-99.1", "EX-101.SCH"]);
        assert!(header.documents[2].is_supporting());
        assert!(!header.documents[1].is_supporting());
    }
}
