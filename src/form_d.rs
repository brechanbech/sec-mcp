//! Form D: notice of an exempt offering of securities.
//!
//! A company or fund raising money without registering the offering, usually
//! under Rule 506, files it within 15 days of the first sale. Private companies
//! and funds file most of them and most have no ticker, which is why this
//! depends on finding filers by name. Funds amend theirs every year, so the
//! newest filing carries the running total sold. Filed as XML since 2009
//! (schema `X0708` today), with no namespace.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::xml::{number, text};

/// The federal exemption codes of Item 6, in plain language. The Investment
/// Company Act exclusions (`3C.1` … `3C.14`) are what let a fund avoid
/// registering as an investment company; 3(c)(1) and 3(c)(7) are the usual two.
fn exemption_label(code: &str) -> Option<String> {
    Some(match code {
        "04" => "Rule 504(b)(1): offerings up to $10 million".into(),
        "04.1" => "Rule 504(b)(1)(i)".into(),
        "04.2" => "Rule 504(b)(1)(ii)".into(),
        "04.3" => "Rule 504(b)(1)(iii)".into(),
        "06b" => "Rule 506(b): private placement, no general solicitation".into(),
        "06c" => "Rule 506(c): general solicitation, all purchasers verified accredited".into(),
        "4a5" => "Securities Act section 4(a)(5)".into(),
        "3C" => "Investment Company Act section 3(c) exclusion".into(),
        "3C.1" => "Investment Company Act 3(c)(1): at most 100 beneficial owners".into(),
        "3C.7" => "Investment Company Act 3(c)(7): qualified purchasers only".into(),
        _ => {
            let n = code.strip_prefix("3C.")?;
            format!("Investment Company Act section 3(c)({n})")
        }
    })
}

/// Item 9's security types, as the flags are named in the schema.
const SECURITY_TYPES: [(&str, &str); 8] = [
    ("isEquityType", "equity"),
    ("isDebtType", "debt"),
    (
        "isOptionToAcquireType",
        "option, warrant or other right to acquire",
    ),
    (
        "isSecurityToBeAcquiredType",
        "security to be acquired on exercise",
    ),
    (
        "isPooledInvestmentFundType",
        "pooled investment fund interests",
    ),
    ("isTenantInCommonType", "tenant-in-common securities"),
    ("isMineralPropertyType", "mineral property securities"),
    ("isOtherType", "other"),
];

fn flag(parent: roxmltree::Node, tag: &str) -> Option<bool> {
    match text(parent, tag)?.as_str() {
        "true" | "Y" => Some(true),
        "false" | "N" => Some(false),
        _ => None,
    }
}

/// A dollar amount that may instead be the word `Indefinite`, as a fund's
/// open-ended offering is: a number when it is one, the filer's word otherwise.
fn amount(parent: roxmltree::Node, tag: &str) -> Value {
    match number(parent, tag) {
        Value::Null => text(parent, tag).map_or(Value::Null, Value::String),
        n => n,
    }
}

/// The text of each `<value>` under the first `tag`, as list items are boxed.
fn values(parent: roxmltree::Node, tag: &str) -> Vec<String> {
    parent
        .descendants()
        .find(|n| n.has_tag_name(tag))
        .into_iter()
        .flat_map(|list| list.children().filter(|c| c.has_tag_name("value")))
        .filter_map(|v| v.text().map(str::trim))
        .filter(|v| !v.is_empty() && *v != "None")
        .map(str::to_owned)
        .collect()
}

fn child<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    parent.descendants().find(|n| n.has_tag_name(tag))
}

fn related_person(node: roxmltree::Node) -> Value {
    let name = child(node, "relatedPersonName").map(|n| {
        ["firstName", "middleName", "lastName"]
            .iter()
            .filter_map(|t| text(n, t))
            .collect::<Vec<_>>()
            .join(" ")
    });
    let relationships: Vec<&str> = node
        .descendants()
        .filter(|n| n.has_tag_name("relationship"))
        .filter_map(|n| n.text().map(str::trim))
        .filter(|r| !r.is_empty())
        .collect();
    let mut person = json!({ "name": name, "relationships": relationships });
    if let Some(c) = text(node, "relationshipClarification") {
        person["clarification"] = json!(c);
    }
    person
}

fn sales_recipient(node: roxmltree::Node) -> Value {
    json!({
        "name": text(node, "recipientName"),
        "crd_number": text(node, "recipientCRDNumber").filter(|c| c != "None"),
        "broker_dealer": text(node, "associatedBDName").filter(|c| c != "None"),
        "states": values(node, "statesOfSolicitationList"),
    })
}

/// Parse a Form D `primary_doc.xml` into the shape the tool returns.
///
/// # Errors
/// If the document is not well-formed XML, or is not a Form D.
pub fn parse(xml: &str) -> Result<Value> {
    let doc = roxmltree::Document::parse(xml)
        .context("Form D was not well-formed XML — EDGAR may have changed the document")?;
    let root = doc.root_element();
    let issuer = child(root, "primaryIssuer").context("not a Form D: no primary issuer")?;
    let offering = child(root, "offeringData").context("not a Form D: no offering data")?;

    let exemptions: Vec<Value> = child(offering, "federalExemptionsExclusions")
        .into_iter()
        .flat_map(|list| list.children().filter(|c| c.has_tag_name("item")))
        .filter_map(|n| n.text().map(str::trim))
        .map(|code| json!({ "code": code, "label": exemption_label(code) }))
        .collect();

    let mut securities: Vec<String> = SECURITY_TYPES
        .iter()
        .filter(|(tag, _)| flag(offering, tag) == Some(true))
        .map(|(_, label)| (*label).to_owned())
        .collect();
    if let Some(other) = text(offering, "descriptionOfOtherType") {
        securities.push(other);
    }

    // The year of incorporation is either a flag or a year.
    let incorporated = child(issuer, "yearOfInc").map(|y| {
        if flag(y, "overFiveYears") == Some(true) {
            json!("over five years ago")
        } else if flag(y, "yetToBeFormed") == Some(true) {
            json!("yet to be formed")
        } else {
            json!(text(y, "value"))
        }
    });

    let first_sale = child(offering, "dateOfFirstSale").map(|d| {
        if flag(d, "yetToOccur") == Some(true) {
            json!("yet to occur")
        } else {
            json!(text(d, "value"))
        }
    });

    let industry = child(offering, "industryGroup");
    let mut out = json!({
        "issuer": {
            "name": text(issuer, "entityName"),
            "cik": text(issuer, "cik"),
            "entity_type": text(issuer, "entityType"),
            "jurisdiction": text(issuer, "jurisdictionOfInc"),
            "incorporated": incorporated,
            "state": child(issuer, "issuerAddress").and_then(|a| text(a, "stateOrCountryDescription")),
            "previous_names": values(issuer, "issuerPreviousNameList"),
        },
        "industry": industry.and_then(|i| text(i, "industryGroupType")),
        "size": child(offering, "issuerSize").and_then(|s| {
            text(s, "revenueRange").or_else(|| text(s, "aggregateNetAssetValueRange"))
        }),
        "exemptions": exemptions,
        "amendment": child(offering, "newOrAmendment").and_then(|n| flag(n, "isAmendment")),
        "date_of_first_sale": first_sale,
        "more_than_one_year": child(offering, "durationOfOffering").and_then(|d| flag(d, "moreThanOneYear")),
        "securities": securities,
        "business_combination": child(offering, "businessCombinationTransaction")
            .and_then(|b| flag(b, "isBusinessCombinationTransaction")),
        "minimum_investment": amount(offering, "minimumInvestmentAccepted"),
        "total_offering_amount": amount(offering, "totalOfferingAmount"),
        "total_amount_sold": amount(offering, "totalAmountSold"),
        "total_remaining": amount(offering, "totalRemaining"),
        "investors": child(offering, "investors").map(|i| json!({
            "total": number(i, "totalNumberAlreadyInvested"),
            "has_non_accredited": flag(i, "hasNonAccreditedInvestors"),
            "non_accredited": number(i, "numberNonAccreditedInvestors"),
        })),
        "sales_commissions": child(offering, "salesCommissions").map_or(Value::Null, |s| amount(s, "dollarAmount")),
        "finders_fees": child(offering, "findersFees").map_or(Value::Null, |s| amount(s, "dollarAmount")),
        // Item 16: proceeds paid to the people in Item 3, executives, directors
        // and promoters.
        "proceeds_to_related_persons": child(offering, "grossProceedsUsed")
            .map_or(Value::Null, |s| amount(s, "dollarAmount")),
        "related_persons": root
            .descendants()
            .filter(|n| n.has_tag_name("relatedPersonInfo"))
            .map(related_person)
            .collect::<Vec<_>>(),
        "sales_compensation": root
            .descendants()
            .filter(|n| n.has_tag_name("recipient"))
            .map(sales_recipient)
            .collect::<Vec<_>>(),
    });
    if let Some(fund) = industry.and_then(|i| child(i, "investmentFundInfo")) {
        out["fund"] = json!({
            "type": text(fund, "investmentFundType"),
            "registered_investment_company": flag(fund, "is40Act"),
        });
    }
    if let Some(c) =
        child(offering, "offeringSalesAmounts").and_then(|o| text(o, "clarificationOfResponse"))
    {
        out["amounts_clarification"] = json!(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SpaceX's Form D of 5 August 2022, accession 0001181412-22-000003,
    /// trimmed to two related persons.
    const SPACEX: &str = r#"<?xml version="1.0"?>
<edgarSubmission>
    <schemaVersion>X0708</schemaVersion>
    <submissionType>D</submissionType>
    <primaryIssuer>
        <cik>0001181412</cik>
        <entityName>SPACE EXPLORATION TECHNOLOGIES CORP</entityName>
        <issuerAddress><stateOrCountryDescription>CALIFORNIA</stateOrCountryDescription></issuerAddress>
        <jurisdictionOfInc>DELAWARE</jurisdictionOfInc>
        <issuerPreviousNameList><value>None</value></issuerPreviousNameList>
        <entityType>Corporation</entityType>
        <yearOfInc><overFiveYears>true</overFiveYears></yearOfInc>
    </primaryIssuer>
    <relatedPersonsList>
        <relatedPersonInfo>
            <relatedPersonName><firstName>ELON</firstName><lastName>MUSK</lastName></relatedPersonName>
            <relatedPersonRelationshipList>
                <relationship>Executive Officer</relationship>
                <relationship>Director</relationship>
            </relatedPersonRelationshipList>
            <relationshipClarification></relationshipClarification>
        </relatedPersonInfo>
        <relatedPersonInfo>
            <relatedPersonName><firstName>GWYNNE</firstName><lastName>SHOTWELL</lastName></relatedPersonName>
            <relatedPersonRelationshipList><relationship>Executive Officer</relationship></relatedPersonRelationshipList>
        </relatedPersonInfo>
    </relatedPersonsList>
    <offeringData>
        <industryGroup><industryGroupType>Other Technology</industryGroupType></industryGroup>
        <issuerSize><revenueRange>Decline to Disclose</revenueRange></issuerSize>
        <federalExemptionsExclusions><item>06b</item></federalExemptionsExclusions>
        <typeOfFiling>
            <newOrAmendment><isAmendment>false</isAmendment></newOrAmendment>
            <dateOfFirstSale><value>2022-07-20</value></dateOfFirstSale>
        </typeOfFiling>
        <durationOfOffering><moreThanOneYear>false</moreThanOneYear></durationOfOffering>
        <typesOfSecuritiesOffered><isEquityType>true</isEquityType></typesOfSecuritiesOffered>
        <businessCombinationTransaction><isBusinessCombinationTransaction>false</isBusinessCombinationTransaction></businessCombinationTransaction>
        <minimumInvestmentAccepted>0</minimumInvestmentAccepted>
        <salesCompensationList></salesCompensationList>
        <offeringSalesAmounts>
            <totalOfferingAmount>249999890</totalOfferingAmount>
            <totalAmountSold>249999890</totalAmountSold>
            <totalRemaining>0</totalRemaining>
            <clarificationOfResponse></clarificationOfResponse>
        </offeringSalesAmounts>
        <investors>
            <hasNonAccreditedInvestors>false</hasNonAccreditedInvestors>
            <totalNumberAlreadyInvested>5</totalNumberAlreadyInvested>
        </investors>
        <salesCommissionsFindersFees>
            <salesCommissions><dollarAmount>0</dollarAmount></salesCommissions>
            <findersFees><dollarAmount>0</dollarAmount></findersFees>
        </salesCommissionsFindersFees>
        <useOfProceeds><grossProceedsUsed><dollarAmount>0</dollarAmount></grossProceedsUsed></useOfProceeds>
    </offeringData>
</edgarSubmission>"#;

    #[test]
    fn a_company_offering_is_read() {
        let v = parse(SPACEX).unwrap();
        assert_eq!(v["issuer"]["cik"], "0001181412");
        assert_eq!(v["issuer"]["incorporated"], "over five years ago");
        // "None" is the form's placeholder, not a previous name.
        assert_eq!(v["issuer"]["previous_names"], json!([]));
        assert_eq!(v["exemptions"][0]["code"], "06b");
        assert_eq!(
            v["exemptions"][0]["label"],
            "Rule 506(b): private placement, no general solicitation"
        );
        assert_eq!(v["date_of_first_sale"], "2022-07-20");
        assert_eq!(v["securities"], json!(["equity"]));
        assert_eq!(v["total_amount_sold"], json!(249_999_890));
        assert_eq!(v["investors"]["total"], json!(5));
        assert_eq!(v["amendment"], json!(false));
        assert_eq!(v["related_persons"][0]["name"], "ELON MUSK");
        assert_eq!(
            v["related_persons"][0]["relationships"],
            json!(["Executive Officer", "Director"])
        );
        assert!(v.get("fund").is_none());
    }

    #[test]
    fn a_fund_offering_reads_indefinite_amounts_and_fund_type() {
        // From Bridgewater Pure Alpha Fund VI's D/A, accession
        // 0000905148-26-001855.
        let fund = SPACEX
            .replace(
                "<industryGroupType>Other Technology</industryGroupType>",
                "<industryGroupType>Pooled Investment Fund</industryGroupType>\
                 <investmentFundInfo><investmentFundType>Other Investment Fund</investmentFundType>\
                 <is40Act>false</is40Act></investmentFundInfo>",
            )
            .replace(
                "<item>06b</item>",
                "<item>06c</item><item>3C</item><item>3C.7</item>",
            )
            .replace(
                "<totalOfferingAmount>249999890</totalOfferingAmount>",
                "<totalOfferingAmount>Indefinite</totalOfferingAmount>",
            );
        let v = parse(&fund).unwrap();
        assert_eq!(v["fund"]["type"], "Other Investment Fund");
        assert_eq!(v["fund"]["registered_investment_company"], json!(false));
        assert_eq!(v["total_offering_amount"], "Indefinite");
        assert_eq!(
            v["exemptions"][2]["label"],
            "Investment Company Act 3(c)(7): qualified purchasers only"
        );
    }

    #[test]
    fn sales_compensation_recipients_are_listed() {
        let xml = SPACEX.replace(
            "<salesCompensationList></salesCompensationList>",
            "<salesCompensationList><recipient>\
             <recipientName>DALMORE GROUP LLC</recipientName>\
             <recipientCRDNumber>136352</recipientCRDNumber>\
             <associatedBDName>DALMORE GROUP LLC</associatedBDName>\
             <statesOfSolicitationList><value>All States</value></statesOfSolicitationList>\
             </recipient></salesCompensationList>",
        );
        let v = parse(&xml).unwrap();
        let r = &v["sales_compensation"][0];
        assert_eq!(r["name"], "DALMORE GROUP LLC");
        assert_eq!(r["crd_number"], "136352");
        assert_eq!(r["states"], json!(["All States"]));
    }

    #[test]
    fn exemption_codes_beyond_the_common_ones_are_spelled_out() {
        assert_eq!(
            exemption_label("3C.5").as_deref(),
            Some("Investment Company Act section 3(c)(5)")
        );
        assert_eq!(exemption_label("zz"), None);
    }

    #[test]
    fn a_document_without_an_issuer_is_refused() {
        assert!(parse("<edgarSubmission><offeringData/></edgarSubmission>").is_err());
    }
}
