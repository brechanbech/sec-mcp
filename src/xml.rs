//! Small readers shared by the EDGAR XML forms that put their values directly
//! in elements: Schedules 13D and 13G and Form 144. (Forms 3, 4 and 5 box most
//! values in `<value>` and have their own readers in `main.rs`.)

use serde_json::{json, Value};

/// Trimmed text of the first descendant named `tag`, or `None` if absent or
/// blank. Matches the local name, so `ns2:issuerCik` and `issuerCik` both count:
/// filing agents differ on whether they prefix the default namespace.
pub fn text(parent: roxmltree::Node, tag: &str) -> Option<String> {
    let t = parent
        .descendants()
        .find(|n| n.has_tag_name(tag))?
        .text()?
        .trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// A share count, amount or percentage. Filers write `1099168953`, `0.00` and
/// `7.48`; whole numbers are returned as integers so counts do not read as
/// `1.0e9`.
pub fn number(parent: roxmltree::Node, tag: &str) -> Value {
    let Some(n) = text(parent, tag).and_then(|t| t.replace(',', "").parse::<f64>().ok()) else {
        return Value::Null;
    };
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// `MM/DD/YYYY`, as these forms write dates, to ISO 8601. Anything else is
/// passed through unchanged rather than guessed at.
pub fn iso_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('/').collect();
    match parts.as_slice() {
        [m, d, y] if m.len() == 2 && d.len() == 2 && y.len() == 4 => format!("{y}-{m}-{d}"),
        _ => date.to_owned(),
    }
}

/// [`text`] read as a date and converted with [`iso_date`].
pub fn date(parent: roxmltree::Node, tag: &str) -> Option<String> {
    text(parent, tag).map(|d| iso_date(&d))
}
