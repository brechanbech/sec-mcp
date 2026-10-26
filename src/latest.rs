//! EDGAR's latest-filings feed: what has been filed across EDGAR, newest first.
//!
//! `https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&output=atom` is
//! the Atom form of EDGAR's "Latest Filings" page. It is real time, carrying
//! each filing's acceptance timestamp, and reaches back about two business days
//! (some 500 8-Ks) before it stops returning entries. `type` filters by form and
//! matches amendments too: `10-K` also returns `10-K/A`.
//!
//! A filing with several parties appears once per party, on adjacent entries
//! with the same accession number: a Form 4 as its reporting person and its
//! issuer, a Schedule 13D as each filer and its subject company. Those are
//! grouped back into one filing here.

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// One feed entry: one party to one filing.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub form: String,
    pub accession: String,
    pub name: String,
    pub cik: Option<String>,
    pub role: Option<String>,
    pub index_url: Option<String>,
    pub accepted: Option<String>,
    pub filed: Option<String>,
    pub size: Option<String>,
    pub items: Vec<String>,
}

/// Decode the feed body. It declares ISO-8859-1 and the response names no
/// charset, so bytes that are not valid UTF-8 are read as Latin-1, where every
/// byte is its own code point.
pub fn decode(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect())
}

/// `Greenwave Technology Solutions, Inc. (0001589149) (Filer)`, the title with
/// its `8-K - ` prefix removed, into name, CIK and role. Taken from the right,
/// since a name can itself contain parentheses.
fn split_party(rest: &str) -> (String, Option<String>, Option<String>) {
    let mut name = rest.trim();
    let mut role = None;
    let mut cik = None;
    if let Some((before, r)) = name.rsplit_once(" (") {
        if let Some(r) = r.strip_suffix(')') {
            role = Some(r.to_owned());
            name = before;
        }
    }
    if let Some((before, c)) = name.rsplit_once(" (") {
        if let Some(c) = c
            .strip_suffix(')')
            .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
        {
            cik = Some(c.to_owned());
            name = before;
        }
    }
    (name.to_owned(), cik, role)
}

/// The summary is escaped HTML: ` <b>Filed:</b> 2026-10-23 <b>AccNo:</b> …
/// <b>Size:</b> 200 KB<br>Item 5.08: …<br>Item 8.01: …`. Items appear on 8-Ks.
fn parse_summary(summary: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut parts = summary.split("<br>");
    let head = parts.next().unwrap_or_default();
    let after = |label: &str| {
        head.split_once(label)
            .map(|(_, rest)| {
                rest.split("<b>")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            })
            .filter(|v| !v.is_empty())
    };
    let items = parts
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    (after("Filed:</b>"), after("Size:</b>"), items)
}

/// Parse the Atom feed into entries, in feed order.
///
/// # Errors
/// If the body is not well-formed XML.
pub fn parse(xml: &str) -> Result<Vec<Entry>> {
    let doc = roxmltree::Document::parse(xml)
        .context("latest-filings feed was not well-formed XML — EDGAR may have changed it")?;
    let child_text = |n: roxmltree::Node, tag: &str| {
        n.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_owned())
    };
    let mut out = Vec::new();
    for entry in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("entry"))
    {
        let form = entry
            .children()
            .find(|c| c.has_tag_name("category"))
            .and_then(|c| c.attribute("term"))
            .unwrap_or_default()
            .to_owned();
        let Some(accession) = child_text(entry, "id").and_then(|id| {
            id.rsplit_once("accession-number=")
                .map(|(_, a)| a.to_owned())
        }) else {
            continue;
        };
        let title = child_text(entry, "title").unwrap_or_default();
        let rest = title.strip_prefix(&format!("{form} - ")).unwrap_or(&title);
        let (name, cik, role) = split_party(rest);
        let (filed, size, items) = parse_summary(&child_text(entry, "summary").unwrap_or_default());
        out.push(Entry {
            form,
            accession,
            name,
            cik,
            role,
            index_url: entry
                .children()
                .find(|c| c.has_tag_name("link"))
                .and_then(|l| l.attribute("href"))
                .map(str::to_owned),
            accepted: child_text(entry, "updated"),
            filed,
            size,
            items,
        });
    }
    Ok(out)
}

/// Group adjacent entries of one accession into filings. Returns the filings
/// and, for each, how many entries it consumed, so a caller can page by entry.
pub fn group(entries: &[Entry]) -> Vec<(Value, usize)> {
    let mut out: Vec<(Value, usize)> = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let first = &entries[i];
        let run = entries[i..]
            .iter()
            .take_while(|e| e.accession == first.accession)
            .count();
        let parties: Vec<Value> = entries[i..i + run]
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "cik": e.cik,
                    "role": e.role,
                })
            })
            .collect();
        let mut filing = json!({
            "form": first.form,
            "accession": first.accession,
            "filed": first.filed,
            "accepted": first.accepted,
            "url": first.index_url,
            "size": first.size,
            "parties": parties,
        });
        if !first.items.is_empty() {
            filing["items"] = json!(first.items);
        }
        out.push((filing, run));
        i += run;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three entries from the feed of 26 October 2026: an 8-K with its items,
    /// and a Form 4 listed once as its reporting person and once as its issuer.
    const FEED: &str = r#"<?xml version="1.0" encoding="ISO-8859-1" ?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Latest Filings - Mon, 26 Oct 2026 04:28:14 EDT</title>
<entry>
<title>8-K - Greenwave Technology Solutions, Inc. (0001589149) (Filer)</title>
<link rel="alternate" type="text/html" href="https://www.sec.gov/Archives/edgar/data/1589149/000149315226048288/0001493152-26-048288-index.htm"/>
<summary type="html">
 &lt;b&gt;Filed:&lt;/b&gt; 2026-10-23 &lt;b&gt;AccNo:&lt;/b&gt; 0001493152-26-048288 &lt;b&gt;Size:&lt;/b&gt; 200 KB
&lt;br&gt;Item 5.08: Shareholder Nominations Pursuant to Exchange Act Rule 14a-11
&lt;br&gt;Item 8.01: Other Events
</summary>
<updated>2026-10-23T17:30:15-04:00</updated>
<category scheme="https://www.sec.gov/" label="form type" term="8-K"/>
<id>urn:tag:sec.gov,2008:accession-number=0001493152-26-048288</id>
</entry>
<entry>
<title>4 - Ruddy Jordan B. (0001575426) (Reporting)</title>
<link rel="alternate" type="text/html" href="https://www.sec.gov/Archives/edgar/data/1575426/000157542626000001/0001575426-26-000001-index.htm"/>
<summary type="html"> &lt;b&gt;Filed:&lt;/b&gt; 2026-10-23 &lt;b&gt;AccNo:&lt;/b&gt; 0001575426-26-000001 &lt;b&gt;Size:&lt;/b&gt; 5 KB</summary>
<updated>2026-10-23T17:29:00-04:00</updated>
<category scheme="https://www.sec.gov/" label="form type" term="4"/>
<id>urn:tag:sec.gov,2008:accession-number=0001575426-26-000001</id>
</entry>
<entry>
<title>4 - Bluerock Private Real Estate Fund (0001551047) (Issuer)</title>
<link rel="alternate" type="text/html" href="https://www.sec.gov/Archives/edgar/data/1551047/000157542626000001/0001575426-26-000001-index.htm"/>
<summary type="html"> &lt;b&gt;Filed:&lt;/b&gt; 2026-10-23 &lt;b&gt;AccNo:&lt;/b&gt; 0001575426-26-000001 &lt;b&gt;Size:&lt;/b&gt; 5 KB</summary>
<updated>2026-10-23T17:29:00-04:00</updated>
<category scheme="https://www.sec.gov/" label="form type" term="4"/>
<id>urn:tag:sec.gov,2008:accession-number=0001575426-26-000001</id>
</entry>
</feed>"#;

    #[test]
    fn entries_are_read_with_their_party_and_items() {
        let entries = parse(FEED).unwrap();
        assert_eq!(entries.len(), 3);
        let e = &entries[0];
        assert_eq!(e.form, "8-K");
        assert_eq!(e.accession, "0001493152-26-048288");
        assert_eq!(e.name, "Greenwave Technology Solutions, Inc.");
        assert_eq!(e.cik.as_deref(), Some("0001589149"));
        assert_eq!(e.role.as_deref(), Some("Filer"));
        assert_eq!(e.filed.as_deref(), Some("2026-10-23"));
        assert_eq!(e.size.as_deref(), Some("200 KB"));
        assert_eq!(e.accepted.as_deref(), Some("2026-10-23T17:30:15-04:00"));
        assert_eq!(
            e.items,
            [
                "Item 5.08: Shareholder Nominations Pursuant to Exchange Act Rule 14a-11",
                "Item 8.01: Other Events"
            ]
        );
    }

    #[test]
    fn a_filings_parties_are_grouped_into_one_filing() {
        let filings = group(&parse(FEED).unwrap());
        assert_eq!(filings.len(), 2);
        let (form4, consumed) = &filings[1];
        assert_eq!(*consumed, 2);
        assert_eq!(form4["form"], "4");
        assert_eq!(form4["parties"][0]["role"], "Reporting");
        assert_eq!(
            form4["parties"][1]["name"],
            "Bluerock Private Real Estate Fund"
        );
        assert!(form4.get("items").is_none());
        assert_eq!(filings[0].0["items"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_name_with_parentheses_keeps_them() {
        assert_eq!(
            split_party("Acme (Holdings) Corp (0000000001) (Filer)"),
            (
                "Acme (Holdings) Corp".to_owned(),
                Some("0000000001".to_owned()),
                Some("Filer".to_owned())
            )
        );
    }

    #[test]
    fn latin1_bytes_decode_to_their_code_points() {
        // "Société" in ISO-8859-1: é is the single byte 0xE9.
        assert_eq!(decode(b"Soci\xe9t\xe9".to_vec()), "Société");
        assert_eq!(decode("Société".as_bytes().to_vec()), "Société");
    }
}
