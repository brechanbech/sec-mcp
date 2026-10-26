# Design: reading filing text (roadmap item 2)

Status: proposed, 26 October 2026. Nothing here is implemented yet.

## Why

Every tool today returns either structured XBRL or a URL. The Finance Agent
Benchmark baseline (`evaluation/runs/2026-10-26-baseline/`) scored 11 of 50,
and most of the rest failed on text the agent could not read:

- guidance in earnings-release exhibits
- KPIs, definitions and risk factors in 10-K prose
- deal terms in 8-Ks

The CIK and response-size fixes in 0.7.0 removed two other blockers. Re-running
the affected questions showed the same wall behind them: the company was now
reachable, but the answer was in text.

Clients cannot fill the gap reliably. Claude Desktop's web fetch read only the
first ~120k of a 10-K's ~430k characters and stopped before the MD&A. It
succeeded only through the desktop app's built-in browser, after a manual
approval. Most MCP clients have no such browser.

## The tool

One new tool, `sec_read_filing` ("Read Filing Document"), taking sec-mcp to 11
tools. It cannot be an extension of `sec_recent_filings`: reading is a
different act from listing, with its own paging, and folding it in would make
every listing call carry reading parameters.

### Input

| Parameter | Meaning |
|---|---|
| `url` | A document or filing URL under `https://www.sec.gov/Archives/edgar/data/`, as every other tool returns them |
| `accession` + `ticker` | Alternative to `url`. `ticker` takes a ticker or a CIK, as everywhere else |
| `document` | Optional file name within the filing, e.g. `d40594dex991.htm` |
| `section` | Optional, e.g. `1A`, `7`, `Part II, Item 1A`, `2.02` |
| `find` | Optional text to search for within the document |
| `offset`, `limit` | Character paging; `limit` defaults to 20,000, max 50,000 |

### Behaviour

- **A filing with no document named** returns the filing's document list:
  type, description, file name and URL. `EX-99.1` is shown as "EX-99.1", so the
  press release is findable. XBRL schema and linkbase files and graphics are
  left out of the list, with a count of what was omitted.
- **A document** returns its text, paged. The first page also carries the
  filing's document list, so reading an 8-K cover leads to its exhibits; this
  is the chain the benchmark's Beat or Miss questions need. It also carries the
  section index, so a caller can jump instead of paging.
- **`section`** returns that section's text, paged within it.
- **`find`** returns up to 10 matches, each with about 400 characters of context
  and its offset, plus a total count. The offset then reads the surrounding
  text. This is how a caller reaches one sentence in a 430k-character 10-K in
  two calls rather than twenty-two pages.
- **Unsupported documents** are refused with a pointer, not guessed at:
  - PDFs and images
  - ownership XML: use `sec_insider_transactions`
  - XBRL instance files: use the XBRL tools

### Output

```json
{
  "company": "Airbnb, Inc.", "cik": "0001559720",
  "accession": "0001559720-25-000010", "form": "10-K", "filed": "2025-02-13",
  "document": { "name": "abnb-20241231.htm", "type": "10-K", "url": "…" },
  "length": 427540, "offset": 0, "returned": 19987, "next_offset": 19987,
  "images": 2,
  "sections": [ { "label": "Part I, Item 1A", "title": "Risk Factors", "offset": 42214 } ],
  "documents": [ { "name": "…", "type": "EX-21.1", "description": "…", "url": "…" } ],
  "text": "…"
}
```

`images` is always reported, and when it is non-zero the response says that
text inside images is not extracted. Airbnb's Q1 2025 shareholder letter
carries 27 full-page images beside its text. Pages break at line boundaries, so
a page never ends mid-row.

## Where the document list comes from

`{accession}-index-headers.html`, about 2 KB, holds the filing's SGML header:
form, filing date, period, and for each document its `TYPE`, `SEQUENCE`,
`FILENAME` and `DESCRIPTION`. `index.json` has file names and sizes but no
types. The full submission `.txt` has everything but is the whole filing. One
small request per filing, verified on the Airbnb Q1 2025 8-K.

## Extraction

**Parser: html5ever's tokenizer, without its tree builder.**
- **Footprint:** it adds 10 crates, all pure Rust and from the Servo project
  apart from `phf`, and no C. That keeps to the crate's stance that `ring` is
  the only C.
- **Why not `tl`:** it adds one crate, but has had no release since January
  2024.
- **Why not `scraper` or `html2text`:** they add 26 and 12 crates and give
  less control over tables.
- **Why not `roxmltree` alone:** it would parse inline-XBRL 10-Ks, which are
  well-formed XHTML, but not agent-prepared exhibits. The Airbnb exhibit fails
  `xmllint` on its first `<img>`.
- **Why the tokenizer is enough:** text extraction needs tag and text events
  in order, not a corrected tree, and the tokenizer copes with malformed HTML.

**Rules, all exercised by the prototype:**

- **Drop what is not shown:** anything under `display:none`, `ix:header`
  (inline XBRL's hidden contexts), `head`, `script`, `style`.
- **SGML wrapper:** agent-prepared documents open with
  `<DOCUMENT><TYPE>EX-99.1<SEQUENCE>2…`, tags that are never closed. Their
  values are skipped without opening an element; treating them as elements hid
  the whole exhibit in the first prototype.
- **Tables become one line per row**, cells joined by ` | `. EDGAR splits
  `$`, `1,234` and `)` or `%` into separate cells, so those are merged back:
  `Asia Pacific | 51 | 11 | 61 | 12 | 20`.
- **Block elements end lines.** Whitespace and non-breaking spaces collapse.
- **Images are counted,** and their text is not seen.
- **Plain-text documents** (older filings) pass through with the SGML wrapper
  stripped.

## Sections

These are headings of the form "Item 1A. Risk Factors". The heuristic:

- Track the current part ("PART II").
- Accept "Item" plus a number, letter or decimal, as a line or as a two-cell
  row.
- Reject table-of-contents rows, which end in a page-number cell.
- Drop headings followed by the next within 150 characters, which is how a
  contents listing looks.

It is best effort and labelled as such. A document with no item headings, such
as a proxy statement or a shareholder letter, gets an empty index and relies on
`find`.

## Prototype results

Run against live documents on 26 October 2026:

| Document | HTML | Text | Time | Sections found |
|---|---|---|---|---|
| Airbnb 10-K FY2024 (inline XBRL) | 2.19 MB | 427,540 chars | 21 ms | 17, all correct, Parts I–IV |
| Airbnb 10-Q Q1 2025 | 1.16 MB | 117,527 | 12 ms | 9; Part II, Item 1 distinct from Part I, Item 1 |
| Airbnb 8-K, 1 May 2025 | 26 KB | 3,875 | <1 ms | Items 2.02 and 9.01 |
| Airbnb EX-99.1 shareholder letter | 57 KB | 45,873 | <1 ms | none (no items); 27 images counted |
| Apple 10-K FY2009 (pre-XBRL HTML) | 1.23 MB | 358,212 | 20 ms | 16, all correct |

Both benchmark answers that Claude Desktop's fetch could not reach are in the
extracted text:
- **The 10-K:** "Average nights per booking in 2024 was … 3.3 for Asia Pacific".
- **The exhibit:** "we expect to generate revenue of $2.99 billion to
  $3.05 billion".

## Caching and limits

- **Cache:** extracted text goes into a small in-memory LRU of 8 documents,
  keyed by URL. Paging through a 10-K then fetches it once, not once per page.
  Filed documents never change, so entries need no expiry.
- **Download cap:** 25 MB, so a pathological document cannot exhaust memory.
- **URL allowlist:** only `https://www.sec.gov/Archives/edgar/data/…` is
  fetched. The URL is model-controlled, and this tool must not become a general
  fetcher.

## Tests

- **Unit tests on small HTML fixtures:**
  - hidden content dropped
  - SGML wrapper skipped
  - `$`/`)`/`%` cell merging
  - table-of-contents rows rejected
  - parts tracked
  - 8-K decimal items read
  - the allowlist refusing other hosts and paths
- **Live suite:**
  - The 8-K's document list contains `EX-99.1`.
  - `find: "Asia Pacific"` on the 10-K returns the 3.3 sentence.
  - `section: "1A"` starts at the risk factors.
  - The exhibit's text contains the guidance.
- **Benchmark:** re-run all 50 as the "after" measurement.

## Open questions

1. **Name.** `sec_read_filing`, or `sec_filing_text`?
2. **Page size.** 20,000 characters is about 5,000 tokens. Larger pages mean
   fewer calls but more context spent per call.
3. **Effort.** The prototype is about 250 lines. The tool with paging, `find`,
   the document list, cache, allowlist and tests should be roughly 600–800
   lines.
