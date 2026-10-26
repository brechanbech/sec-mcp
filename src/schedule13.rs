//! Schedules 13D and 13G: who owns more than 5% of a class of a company's stock.
//!
//! Both have been filed as structured XML since 18 December 2024 (schema
//! `X0202`), under the forms `SCHEDULE 13D` and `SCHEDULE 13G`. A 13G is the
//! short form for passive and institutional holders; a 13D is the long form for
//! anyone who may seek to influence control, and its Item 4 says why they bought.
//! Filings before that date are `SC 13D` and `SC 13G`, in HTML or plain text,
//! and are listed for `sec_filing_text` to read rather than scraped here.

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// Item texts in a 13D run from a sentence to several pages. 4,000 characters
/// holds most purposes whole and keeps a page of ten filings readable.
const ITEM_TEXT_CAP: usize = 4_000;
/// Cover-page comments are usually a sentence of qualification.
const COMMENT_CAP: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    D,
    G,
}

impl Schedule {
    pub fn label(self) -> &'static str {
        match self {
            Schedule::D => "13D",
            Schedule::G => "13G",
        }
    }
}

/// Classify a submissions form label. Returns the schedule and whether the
/// filing is structured XML (`SCHEDULE 13G`) rather than the older HTML or
/// text (`SC 13G`). Amendments (`/A`) are the same schedule.
pub fn classify_form(form: &str) -> Option<(Schedule, bool)> {
    let base = form.strip_suffix("/A").unwrap_or(form);
    match base {
        "SCHEDULE 13D" => Some((Schedule::D, true)),
        "SCHEDULE 13G" => Some((Schedule::G, true)),
        "SC 13D" => Some((Schedule::D, false)),
        "SC 13G" => Some((Schedule::G, false)),
        _ => None,
    }
}

/// The reporting-person type codes from the cover page instructions.
fn person_type_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "BD" => "broker-dealer",
        "BK" => "bank",
        "IC" => "insurance company",
        "IV" => "registered investment company",
        "IA" => "investment adviser",
        "EP" => "employee benefit plan or endowment fund",
        "HC" => "parent holding company or control person",
        "SA" => "savings association",
        "CP" => "church plan",
        "CO" => "corporation",
        "PN" => "partnership",
        "IN" => "individual",
        "FI" => "non-US institution",
        "OO" => "other",
        _ => return None,
    })
}

/// A 13D's source-of-funds codes (cover page row 4).
fn fund_source_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "SC" => "subject company",
        "BK" => "bank",
        "AF" => "affiliate",
        "WC" => "working capital",
        "PF" => "personal funds",
        "OO" => "other",
        _ => return None,
    })
}

/// Which Rule 13d-1 paragraph a 13G is filed under, which is what makes the
/// holder eligible for the short form.
fn rule_label(rule: &str) -> Option<&'static str> {
    Some(match rule.trim() {
        "Rule 13d-1(b)" => "qualified institutional investor",
        "Rule 13d-1(c)" => "passive investor, under 20%",
        "Rule 13d-1(d)" => "exempt investor, e.g. holding since before registration",
        _ => return None,
    })
}

/// Trimmed text of the first descendant named `tag`, or `None` if absent or
/// blank.
fn text(parent: roxmltree::Node, tag: &str) -> Option<String> {
    let t = parent
        .descendants()
        .find(|n| n.has_tag_name(tag))?
        .text()?
        .trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// A share count or percentage. Filers write `1099168953`, `0.00` and `7.48`;
/// whole numbers are returned as integers so counts do not read as `1.0e9`.
fn number(parent: roxmltree::Node, tag: &str) -> Value {
    let Some(n) = text(parent, tag).and_then(|t| t.replace(',', "").parse::<f64>().ok()) else {
        return Value::Null;
    };
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// `MM/DD/YYYY`, as the schedules write dates, to ISO 8601. Anything else is
/// passed through unchanged rather than guessed at.
fn iso_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('/').collect();
    match parts.as_slice() {
        [m, d, y] if m.len() == 2 && d.len() == 2 && y.len() == 4 => format!("{y}-{m}-{d}"),
        _ => date.to_owned(),
    }
}

/// Cap a free-text item, marking the cut so it does not read as the whole.
fn capped(s: String, cap: usize) -> (String, bool) {
    if s.chars().count() <= cap {
        return (s, false);
    }
    let cut: String = s.chars().take(cap).collect();
    (format!("{}…", cut.trim_end()), true)
}

fn set_text(out: &mut Value, key: &str, value: Option<String>, cap: usize) {
    if let Some(v) = value {
        let (v, truncated) = capped(v, cap);
        out[key] = json!(v);
        if truncated {
            out[format!("{key}_truncated")] = json!(true);
        }
    }
}

fn coded(code: &str, label: Option<&'static str>) -> Value {
    json!({ "code": code, "label": label })
}

/// One cover page: a reporting person and what they own.
///
/// The two schedules name the same facts differently (`classPercent` against
/// `percentOfClass`, `comments` against `commentContent`), so both spellings
/// are read.
fn reporting_person(node: roxmltree::Node, schedule: Schedule) -> Value {
    let kind = text(node, "typeOfReportingPerson");
    let mut person = json!({
        "name": text(node, "reportingPersonName"),
        "cik": text(node, "reportingPersonCIK"),
        "type": kind.as_deref().map(|k| coded(k, person_type_label(k))),
        "citizenship_or_organization": text(node, "citizenshipOrOrganization"),
        "sole_voting_power": number(node, "soleVotingPower"),
        "shared_voting_power": number(node, "sharedVotingPower"),
        "sole_dispositive_power": number(node, "soleDispositivePower"),
        "shared_dispositive_power": number(node, "sharedDispositivePower"),
        "aggregate_shares_owned": match schedule {
            Schedule::D => number(node, "aggregateAmountOwned"),
            Schedule::G => number(node, "reportingPersonBeneficiallyOwnedAggregateNumberOfShares"),
        },
        "percent_of_class": match schedule {
            Schedule::D => number(node, "percentOfClass"),
            Schedule::G => number(node, "classPercent"),
        },
    });
    if schedule == Schedule::D {
        let sources: Vec<Value> = node
            .children()
            .filter(|n| n.has_tag_name("fundType"))
            .filter_map(|n| n.text().map(str::trim))
            .filter(|c| !c.is_empty())
            .map(|c| coded(c, fund_source_label(c)))
            .collect();
        person["fund_sources"] = json!(sources);
    }
    let comment = text(node, "commentContent").or_else(|| text(node, "comments"));
    set_text(&mut person, "comment", comment, COMMENT_CAP);
    person
}

/// Parse a Schedule 13D or 13G `primary_doc.xml` into the shape the tool
/// returns.
///
/// # Errors
/// If the document is not well-formed XML, or is neither schedule.
pub fn parse(xml: &str) -> Result<Value> {
    let doc = roxmltree::Document::parse(xml)
        .context("schedule was not well-formed XML — EDGAR may have changed the document")?;
    let root = doc.root_element();
    let schedule = match root.tag_name().namespace() {
        Some(ns) if ns.eq_ignore_ascii_case("http://www.sec.gov/edgar/schedule13D") => Schedule::D,
        Some(ns) if ns.eq_ignore_ascii_case("http://www.sec.gov/edgar/schedule13g") => Schedule::G,
        other => anyhow::bail!(
            "not a Schedule 13D or 13G document (namespace {})",
            other.unwrap_or("none")
        ),
    };
    let cover = root
        .descendants()
        .find(|n| n.has_tag_name("coverPageHeader"))
        .context("schedule has no cover page")?;

    let person_tag = match schedule {
        Schedule::D => "reportingPersonInfo",
        Schedule::G => "coverPageHeaderReportingPersonDetails",
    };
    let persons: Vec<Value> = root
        .descendants()
        .filter(|n| n.has_tag_name(person_tag))
        .map(|n| reporting_person(n, schedule))
        .collect();

    let cusips: Vec<&str> = cover
        .descendants()
        .filter(|n| n.has_tag_name("issuerCusipNumber"))
        .filter_map(|n| n.text().map(str::trim))
        .filter(|c| !c.is_empty())
        .collect();

    let event_date = match schedule {
        Schedule::D => text(cover, "dateOfEvent"),
        Schedule::G => text(cover, "eventDateRequiresFilingThisStatement"),
    };

    let mut out = json!({
        "schedule": schedule.label(),
        "amendment_no": number(cover, "amendmentNo"),
        "event_date": event_date.as_deref().map(iso_date),
        "security_class": text(cover, "securitiesClassTitle"),
        "issuer": {
            "name": text(cover, "issuerName"),
            "cik": text(cover, "issuerCik").or_else(|| text(cover, "issuerCIK")),
            "cusips": cusips,
        },
        "reporting_persons": persons,
    });

    match schedule {
        Schedule::G => {
            let rules: Vec<Value> = cover
                .descendants()
                .filter(|n| n.has_tag_name("designateRulePursuantThisScheduleFiled"))
                .filter_map(|n| n.text().map(str::trim))
                .filter(|r| !r.is_empty())
                .map(|r| json!({ "rule": r, "label": rule_label(r) }))
                .collect();
            out["filed_under"] = json!(rules);
            // Item 5: the holder has dropped to 5% or less, so this is their
            // last filing on the class until they cross 5% again.
            if let Some(item5) = root.descendants().find(|n| n.has_tag_name("item5")) {
                if text(item5, "classOwnership5PercentOrLess").as_deref() == Some("Y") {
                    out["exit_five_percent_or_less"] = json!(true);
                }
            }
        }
        Schedule::D => {
            set_text(
                &mut out,
                "source_of_funds",
                text(root, "fundsSource"),
                ITEM_TEXT_CAP,
            );
            set_text(
                &mut out,
                "purpose",
                text(root, "transactionPurpose"),
                ITEM_TEXT_CAP,
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vanguard Capital Management's 13G on Apple, accession
    /// 0002100119-26-000139 (filed 29 April 2026), trimmed.
    const G: &str = r#"<?xml version="1.0" encoding="UTF-8"?><edgarSubmission xmlns="http://www.sec.gov/edgar/schedule13g" xmlns:com="http://www.sec.gov/edgar/common">
<schemaVersion>X0202</schemaVersion>
<headerData><submissionType>SCHEDULE 13G</submissionType></headerData>
<formData>
<coverPageHeader>
<securitiesClassTitle>Common Stock</securitiesClassTitle>
<eventDateRequiresFilingThisStatement>03/31/2026</eventDateRequiresFilingThisStatement>
<issuerInfo>
<issuerCik>0000320193</issuerCik>
<issuerName>Apple Inc</issuerName>
<issuerCusips><issuerCusipNumber>037833100</issuerCusipNumber></issuerCusips>
<issuerPrincipalExecutiveOfficeAddress><com:city>Cupertino</com:city></issuerPrincipalExecutiveOfficeAddress>
</issuerInfo>
<designateRulesPursuantThisScheduleFiled>
<designateRulePursuantThisScheduleFiled>Rule 13d-1(b)</designateRulePursuantThisScheduleFiled>
</designateRulesPursuantThisScheduleFiled>
</coverPageHeader>
<coverPageHeaderReportingPersonDetails>
<reportingPersonName>Vanguard Capital Management</reportingPersonName>
<citizenshipOrOrganization>PA</citizenshipOrOrganization>
<reportingPersonBeneficiallyOwnedNumberOfShares>
<soleVotingPower>145321305</soleVotingPower>
<sharedVotingPower>0</sharedVotingPower>
<soleDispositivePower>1099168953</soleDispositivePower>
<sharedDispositivePower>0</sharedDispositivePower>
</reportingPersonBeneficiallyOwnedNumberOfShares>
<reportingPersonBeneficiallyOwnedAggregateNumberOfShares>1099168953</reportingPersonBeneficiallyOwnedAggregateNumberOfShares>
<classPercent>7.48</classPercent>
<typeOfReportingPerson>IA</typeOfReportingPerson>
</coverPageHeaderReportingPersonDetails>
<items><item5><notApplicableFlag>Y</notApplicableFlag></item5></items>
</formData>
</edgarSubmission>"#;

    /// Coliseum Capital's 13D on Sonos, accession 0001193125-26-330596 (filed
    /// 3 August 2026), trimmed to one of its six reporting persons.
    const D: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edgarSubmission xmlns="http://www.sec.gov/edgar/schedule13D" xmlns:com="http://www.sec.gov/edgar/common">
  <schemaVersion>X0202</schemaVersion>
  <formData>
    <coverPageHeader>
      <securitiesClassTitle>Common Stock, $0.001 par value per share</securitiesClassTitle>
      <dateOfEvent>07/24/2026</dateOfEvent>
      <issuerInfo>
        <issuerCIK>0001314727</issuerCIK>
        <issuerCusips><issuerCusipNumber>83570H108</issuerCusipNumber></issuerCusips>
        <issuerName>Sonos, Inc.</issuerName>
      </issuerInfo>
    </coverPageHeader>
    <reportingPersons>
      <reportingPersonInfo>
        <reportingPersonCIK>0001409751</reportingPersonCIK>
        <reportingPersonNoCIK>N</reportingPersonNoCIK>
        <reportingPersonName>Coliseum Capital Management, LLC</reportingPersonName>
        <memberOfGroup>b</memberOfGroup>
        <fundType>AF</fundType>
        <legalProceedings>N</legalProceedings>
        <citizenshipOrOrganization>DE</citizenshipOrOrganization>
        <soleVotingPower>0.00</soleVotingPower>
        <sharedVotingPower>12345678.00</sharedVotingPower>
        <soleDispositivePower>0.00</soleDispositivePower>
        <sharedDispositivePower>12345678.00</sharedDispositivePower>
        <aggregateAmountOwned>12345678.00</aggregateAmountOwned>
        <isAggregateExcludeShares>N</isAggregateExcludeShares>
        <percentOfClass>10.2</percentOfClass>
        <typeOfReportingPerson>IA</typeOfReportingPerson>
      </reportingPersonInfo>
    </reportingPersons>
    <items1To7>
      <item3><fundsSource>Working Capital of CCP and CCC IV.</fundsSource></item3>
      <item4><transactionPurpose>The Reporting Persons acquired the Common Stock for investment purposes.</transactionPurpose></item4>
    </items1To7>
  </formData>
</edgarSubmission>"#;

    #[test]
    fn forms_are_classified_by_schedule_and_era() {
        assert_eq!(classify_form("SCHEDULE 13G"), Some((Schedule::G, true)));
        assert_eq!(classify_form("SCHEDULE 13D/A"), Some((Schedule::D, true)));
        assert_eq!(classify_form("SC 13G/A"), Some((Schedule::G, false)));
        assert_eq!(classify_form("SC 13D"), Some((Schedule::D, false)));
        assert_eq!(classify_form("SC TO-T"), None);
        assert_eq!(classify_form("13F-HR"), None);
    }

    #[test]
    fn a_13g_cover_page_is_read() {
        let v = parse(G).unwrap();
        assert_eq!(v["schedule"], "13G");
        assert_eq!(v["event_date"], "2026-03-31");
        assert_eq!(v["issuer"]["cik"], "0000320193");
        assert_eq!(v["issuer"]["cusips"], json!(["037833100"]));
        assert_eq!(
            v["filed_under"][0]["label"],
            "qualified institutional investor"
        );
        let p = &v["reporting_persons"][0];
        assert_eq!(p["name"], "Vanguard Capital Management");
        assert_eq!(p["aggregate_shares_owned"], json!(1_099_168_953_i64));
        assert_eq!(p["percent_of_class"], json!(7.48));
        assert_eq!(p["type"]["label"], "investment adviser");
        assert!(v.get("exit_five_percent_or_less").is_none());
    }

    #[test]
    fn a_13g_exit_is_flagged() {
        let exit = G.replace(
            "<item5><notApplicableFlag>Y</notApplicableFlag></item5>",
            "<item5><notApplicableFlag>N</notApplicableFlag>\
             <classOwnership5PercentOrLess>Y</classOwnership5PercentOrLess></item5>",
        );
        assert_eq!(
            parse(&exit).unwrap()["exit_five_percent_or_less"],
            json!(true)
        );
    }

    #[test]
    fn a_13d_carries_funds_purpose_and_fund_sources() {
        let v = parse(D).unwrap();
        assert_eq!(v["schedule"], "13D");
        assert_eq!(v["event_date"], "2026-07-24");
        // The 13D spells the issuer's CIK element `issuerCIK`.
        assert_eq!(v["issuer"]["cik"], "0001314727");
        let p = &v["reporting_persons"][0];
        assert_eq!(p["cik"], "0001409751");
        // `12345678.00` is a share count, not a fraction.
        assert_eq!(p["shared_voting_power"], json!(12_345_678_i64));
        assert_eq!(p["percent_of_class"], json!(10.2));
        assert_eq!(p["fund_sources"][0]["label"], "affiliate");
        assert!(v["purpose"]
            .as_str()
            .unwrap()
            .contains("investment purposes"));
        assert_eq!(v["source_of_funds"], "Working Capital of CCP and CCC IV.");
    }

    #[test]
    fn a_long_purpose_is_capped_and_marked() {
        let long = "x".repeat(ITEM_TEXT_CAP + 50);
        let xml = D.replace(
            "The Reporting Persons acquired the Common Stock for investment purposes.",
            &long,
        );
        let v = parse(&xml).unwrap();
        assert_eq!(v["purpose_truncated"], json!(true));
        assert_eq!(
            v["purpose"].as_str().unwrap().chars().count(),
            ITEM_TEXT_CAP + 1
        );
    }

    #[test]
    fn another_xml_document_is_refused() {
        let form4 = r#"<?xml version="1.0"?><ownershipDocument><documentType>4</documentType></ownershipDocument>"#;
        assert!(parse(form4).is_err());
    }
}
