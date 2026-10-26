//! Form 13F: an institutional manager's quarterly report of its holdings.
//!
//! Managers with discretion over $100 million or more in US-listed equities
//! file a 13F-HR within 45 days of each quarter end. Its cover page
//! (`primary_doc.xml`) carries the totals; its information table (an XML file
//! named by the filing agent, typed `INFORMATION TABLE` in the filing header)
//! has one row per position per combination of investment discretion and
//! sub-manager, so one holding can span a dozen rows. Rows are aggregated here
//! into positions by CUSIP, class and put/call.
//!
//! Holdings are keyed by CUSIP, which is licensed, and the table carries no
//! ticker, so issuers are named as filed ("APPLE INC"). Values have been in
//! dollars since 3 January 2023 and were in thousands before.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::xml::text;

/// Filings made before this date report `value` in thousands of dollars.
pub const DOLLARS_SINCE: &str = "2023-01-03";

/// One position: every table row for one CUSIP, class and put/call, summed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Position {
    pub issuer: String,
    pub class: String,
    pub cusip: String,
    pub put_call: Option<String>,
    pub value: f64,
    pub amount: f64,
    pub amount_type: Option<String>,
    pub sole: f64,
    pub shared: f64,
    pub none: f64,
    pub discretion: Vec<String>,
    pub rows: usize,
}

impl Position {
    /// Whether this position answers `query`: a CUSIP exactly, or any part of
    /// the issuer's name, ignoring case.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim();
        self.cusip.eq_ignore_ascii_case(q)
            || self
                .issuer
                .to_ascii_lowercase()
                .contains(&q.to_ascii_lowercase())
    }

    pub fn to_json(&self, portfolio_value: f64) -> Value {
        let pct = if portfolio_value > 0.0 {
            (self.value / portfolio_value * 10_000.0).round() / 100.0
        } else {
            0.0
        };
        let mut v = json!({
            "issuer": self.issuer,
            "class": self.class,
            "cusip": self.cusip,
            "value": whole(self.value),
            "percent_of_portfolio": pct,
            "amount": whole(self.amount),
            "amount_type": match self.amount_type.as_deref() {
                Some("SH") => "shares",
                Some("PRN") => "principal",
                _ => "unknown",
            },
            "voting_authority": {
                "sole": whole(self.sole),
                "shared": whole(self.shared),
                "none": whole(self.none),
            },
            "investment_discretion": self.discretion,
        });
        if let Some(pc) = &self.put_call {
            v["put_call"] = json!(pc);
        }
        v
    }
}

/// A whole number as an integer, so a share count does not read as `1.2e7`.
fn whole(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn num(node: roxmltree::Node, tag: &str) -> f64 {
    text(node, tag)
        .and_then(|t| t.replace(',', "").parse().ok())
        .unwrap_or(0.0)
}

/// Parse an information table into positions, largest value first.
/// `value_multiplier` is 1000 for filings made before [`DOLLARS_SINCE`].
///
/// # Errors
/// If the table is not well-formed XML.
pub fn parse_table(xml: &str, value_multiplier: f64) -> Result<Vec<Position>> {
    let doc = roxmltree::Document::parse(xml)
        .context("13F information table was not well-formed XML — EDGAR may have changed it")?;
    let mut index: HashMap<(String, String, Option<String>), usize> = HashMap::new();
    let mut positions: Vec<Position> = Vec::new();
    for row in doc.descendants().filter(|n| n.has_tag_name("infoTable")) {
        let cusip = text(row, "cusip").unwrap_or_default().to_ascii_uppercase();
        let class = text(row, "titleOfClass").unwrap_or_default();
        let put_call = text(row, "putCall");
        let key = (cusip.clone(), class.clone(), put_call.clone());
        let i = *index.entry(key).or_insert_with(|| {
            positions.push(Position {
                issuer: text(row, "nameOfIssuer").unwrap_or_default(),
                class,
                cusip,
                put_call,
                amount_type: text(row, "sshPrnamtType"),
                ..Default::default()
            });
            positions.len() - 1
        });
        let p = &mut positions[i];
        p.value += num(row, "value") * value_multiplier;
        p.amount += num(row, "sshPrnamt");
        if let Some(voting) = row.children().find(|n| n.has_tag_name("votingAuthority")) {
            p.sole += num(voting, "Sole");
            p.shared += num(voting, "Shared");
            p.none += num(voting, "None");
        }
        if let Some(d) = text(row, "investmentDiscretion") {
            if !p.discretion.contains(&d) {
                p.discretion.push(d);
            }
        }
        p.rows += 1;
    }
    positions.sort_by(|a, b| b.value.total_cmp(&a.value));
    Ok(positions)
}

/// The cover page and summary: what the filing says about itself.
///
/// # Errors
/// If the document is not well-formed XML.
pub fn parse_cover(xml: &str, value_multiplier: f64) -> Result<Value> {
    let doc = roxmltree::Document::parse(xml)
        .context("13F cover page was not well-formed XML — EDGAR may have changed it")?;
    let root = doc.root_element();
    let mut out = json!({
        "report_type": text(root, "reportType"),
        "table_entries": text(root, "tableEntryTotal").and_then(|t| t.parse::<u64>().ok()),
        "table_value_total": text(root, "tableValueTotal")
            .and_then(|t| t.replace(',', "").parse::<f64>().ok())
            .map(|v| whole(v * value_multiplier)),
        "other_managers_included": text(root, "otherIncludedManagersCount")
            .and_then(|t| t.parse::<u64>().ok()),
    });
    if text(root, "isAmendment").as_deref() == Some("true") {
        out["amendment_type"] = json!(text(root, "amendmentType"));
    }
    // Confidential treatment lets a manager leave positions out for a time,
    // so the table may not be the whole portfolio.
    if text(root, "isConfidentialOmitted").as_deref() == Some("true") {
        out["confidential_positions_omitted"] = json!(true);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three rows of Berkshire's 13F-HR for 30 June 2026 (accession
    /// 0001193125-26-352200), two of them the same Ally position held through
    /// different sub-managers, plus a hypothetical put on the same CUSIP.
    const TABLE: &str = r#"<informationTable xmlns="http://www.sec.gov/edgar/document/thirteenf/informationtable">
  <infoTable>
    <nameOfIssuer>ALLY FINL INC</nameOfIssuer><titleOfClass>COM</titleOfClass><cusip>02005N100</cusip>
    <value>577211815</value>
    <shrsOrPrnAmt><sshPrnamt>12561737</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
    <investmentDiscretion>DFND</investmentDiscretion><otherManager>4</otherManager>
    <votingAuthority><Sole>12561737</Sole><Shared>0</Shared><None>0</None></votingAuthority>
  </infoTable>
  <infoTable>
    <nameOfIssuer>ALLY FINL INC</nameOfIssuer><titleOfClass>COM</titleOfClass><cusip>02005N100</cusip>
    <value>128838056</value>
    <shrsOrPrnAmt><sshPrnamt>2803875</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
    <investmentDiscretion>DFND</investmentDiscretion><otherManager>2,4,11</otherManager>
    <votingAuthority><Sole>2803875</Sole><Shared>0</Shared><None>0</None></votingAuthority>
  </infoTable>
  <infoTable>
    <nameOfIssuer>ALLY FINL INC</nameOfIssuer><titleOfClass>COM</titleOfClass><cusip>02005N100</cusip>
    <value>1000</value>
    <shrsOrPrnAmt><sshPrnamt>100</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
    <putCall>Put</putCall>
    <investmentDiscretion>SOLE</investmentDiscretion>
    <votingAuthority><Sole>0</Sole><Shared>0</Shared><None>100</None></votingAuthority>
  </infoTable>
</informationTable>"#;

    #[test]
    fn rows_of_one_position_are_summed() {
        let positions = parse_table(TABLE, 1.0).unwrap();
        assert_eq!(positions.len(), 2, "the put is its own position");
        let ally = &positions[0];
        assert_eq!(ally.rows, 2);
        assert_eq!(ally.value, 706_049_871.0);
        assert_eq!(ally.amount, 15_365_612.0);
        assert_eq!(ally.sole, 15_365_612.0);
        assert_eq!(ally.discretion, ["DFND"]);
        assert_eq!(positions[1].put_call.as_deref(), Some("Put"));
    }

    #[test]
    fn values_in_thousands_are_converted() {
        let positions = parse_table(TABLE, 1000.0).unwrap();
        assert_eq!(positions[0].value, 706_049_871_000.0);
        // Share counts were never in thousands.
        assert_eq!(positions[0].amount, 15_365_612.0);
    }

    #[test]
    fn a_position_matches_by_name_or_cusip() {
        let ally = &parse_table(TABLE, 1.0).unwrap()[0];
        assert!(ally.matches("ally"));
        assert!(ally.matches("02005n100"));
        assert!(!ally.matches("apple"));
    }

    #[test]
    fn a_position_reports_its_share_of_the_portfolio() {
        let ally = &parse_table(TABLE, 1.0).unwrap()[0];
        let v = ally.to_json(ally.value * 4.0);
        assert_eq!(v["percent_of_portfolio"], json!(25.0));
        assert_eq!(v["amount_type"], "shares");
        assert_eq!(v["value"], json!(706_049_871_i64));
        assert!(v.get("put_call").is_none());
    }

    #[test]
    fn the_cover_page_gives_totals_and_omissions() {
        let cover = r#"<edgarSubmission xmlns="http://www.sec.gov/edgar/thirteenffiler"><formData>
            <coverPage><reportCalendarOrQuarter>06-30-2026</reportCalendarOrQuarter>
            <isAmendment>false</isAmendment><reportType>13F HOLDINGS REPORT</reportType></coverPage>
            <summaryPage><otherIncludedManagersCount>14</otherIncludedManagersCount>
            <tableEntryTotal>89</tableEntryTotal><tableValueTotal>299253556246</tableValueTotal>
            <isConfidentialOmitted>true</isConfidentialOmitted></summaryPage>
        </formData></edgarSubmission>"#;
        let v = parse_cover(cover, 1.0).unwrap();
        assert_eq!(v["table_entries"], json!(89));
        assert_eq!(v["table_value_total"], json!(299_253_556_246_i64));
        assert_eq!(v["confidential_positions_omitted"], json!(true));
        assert!(v.get("amendment_type").is_none());
    }
}
