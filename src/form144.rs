//! Form 144: notice of a proposed sale of restricted or control securities.
//!
//! An affiliate (an officer, director or large holder) selling under Rule 144
//! files it when they place the order: more than 5,000 shares or $50,000 in
//! three months. It is a notice of intent, not a report of a trade. The
//! executed sale follows on a Form 4, usually within days, which is why it
//! belongs beside Forms 3, 4 and 5. Filed as XML since 13 April 2023, in the
//! ownership namespace, though under its own `edgarSubmission` schema.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::xml::{date, iso_date, number, text};

/// One block of the notice, once per occurrence: a notice can list several
/// lots of shares to be sold, acquired in several ways.
fn each<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    tag: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    root.descendants().filter(move |n| n.has_tag_name(tag))
}

fn proposed_sale(node: roxmltree::Node) -> Value {
    json!({
        "security_class": text(node, "securitiesClassTitle"),
        "shares": number(node, "noOfUnitsSold"),
        "aggregate_market_value": number(node, "aggregateMarketValue"),
        "shares_outstanding": number(node, "noOfUnitsOutstanding"),
        "approximate_sale_date": date(node, "approxSaleDate"),
        "exchange": text(node, "securitiesExchangeName"),
        "broker": each(node, "brokerOrMarketmakerDetails").next().and_then(|b| text(b, "name")),
    })
}

fn acquisition(node: roxmltree::Node) -> Value {
    json!({
        "security_class": text(node, "securitiesClassTitle"),
        "acquired_date": date(node, "acquiredDate"),
        "nature": text(node, "natureOfAcquisitionTransaction"),
        "acquired_from": text(node, "nameOfPersonfromWhomAcquired"),
        "gift": text(node, "isGiftTransaction").as_deref() == Some("Y"),
        "shares": number(node, "amountOfSecuritiesAcquired"),
        "payment_date": date(node, "paymentDate"),
        "nature_of_payment": text(node, "natureOfPayment"),
    })
}

fn past_sale(node: roxmltree::Node) -> Value {
    json!({
        "seller": each(node, "sellerDetails").next().and_then(|s| text(s, "name")),
        "security_class": text(node, "securitiesClassTitle"),
        "sale_date": date(node, "saleDate"),
        "shares": number(node, "amountOfSecuritiesSold"),
        "gross_proceeds": number(node, "grossProceeds"),
    })
}

/// Parse a Form 144 `primary_doc.xml` into the shape the tool returns.
///
/// # Errors
/// If the document is not well-formed XML, or is not a Form 144.
pub fn parse(xml: &str) -> Result<Value> {
    let doc = roxmltree::Document::parse(xml)
        .context("Form 144 was not well-formed XML — EDGAR may have changed the document")?;
    let root = doc.root_element();
    let issuer = each(root, "issuerInfo")
        .next()
        .context("not a Form 144: no issuer information")?;

    let relationships: Vec<String> = each(issuer, "relationshipToIssuer")
        .filter_map(|n| n.text().map(str::trim))
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    let plan_dates: Vec<String> = each(root, "planAdoptionDate")
        .filter_map(|n| n.text())
        .map(|d| iso_date(d.trim()))
        .collect();

    let mut out = json!({
        "issuer": {
            "name": text(issuer, "issuerName"),
            "cik": text(issuer, "issuerCik"),
        },
        "seller": {
            "name": text(issuer, "nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold"),
            "relationship": relationships,
        },
        "proposed_sales": each(root, "securitiesInformation").map(proposed_sale).collect::<Vec<_>>(),
        "acquired": each(root, "securitiesToBeSold").map(acquisition).collect::<Vec<_>>(),
        "sold_past_3_months": each(root, "securitiesSoldInPast3Months").map(past_sale).collect::<Vec<_>>(),
        "notice_date": date(root, "noticeDate"),
    });
    // A 10b5-1 plan adopted in advance is the usual defence that the seller
    // was not trading on inside information, so its date matters.
    if !plan_dates.is_empty() {
        out["rule_10b5_1_plan_adopted"] = json!(plan_dates);
    }
    if let Some(remarks) = text(root, "remarks") {
        out["remarks"] = json!(remarks);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// John Ternus's notice on Apple, accession 0001958244-26-000636 (filed
    /// 2 October 2026), trimmed. The filing agent prefixes every element.
    const TERNUS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ns2:edgarSubmission xmlns:com="http://www.sec.gov/edgar/common" xmlns:ns2="http://www.sec.gov/edgar/ownership">
<ns2:headerData><ns2:submissionType>144</ns2:submissionType></ns2:headerData>
<ns2:formData>
<ns2:issuerInfo>
<ns2:issuerCik>0000320193</ns2:issuerCik>
<ns2:issuerName>Apple Inc.</ns2:issuerName>
<ns2:nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold>John Ternus</ns2:nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold>
<ns2:relationshipsToIssuer><ns2:relationshipToIssuer>Officer</ns2:relationshipToIssuer></ns2:relationshipsToIssuer>
</ns2:issuerInfo>
<ns2:securitiesInformation>
<ns2:securitiesClassTitle>Common</ns2:securitiesClassTitle>
<ns2:brokerOrMarketmakerDetails><ns2:name>Goldman Sachs &amp; Co. LLC</ns2:name></ns2:brokerOrMarketmakerDetails>
<ns2:noOfUnitsSold>25412</ns2:noOfUnitsSold>
<ns2:aggregateMarketValue>8479730.28</ns2:aggregateMarketValue>
<ns2:noOfUnitsOutstanding>14594180000</ns2:noOfUnitsOutstanding>
<ns2:approxSaleDate>10/02/2026</ns2:approxSaleDate>
<ns2:securitiesExchangeName>NASD</ns2:securitiesExchangeName>
</ns2:securitiesInformation>
<ns2:securitiesToBeSold>
<ns2:securitiesClassTitle>Common</ns2:securitiesClassTitle>
<ns2:acquiredDate>10/01/2026</ns2:acquiredDate>
<ns2:natureOfAcquisitionTransaction>Acquired as compensation -- Restricted Stock Units</ns2:natureOfAcquisitionTransaction>
<ns2:nameOfPersonfromWhomAcquired>Issuer</ns2:nameOfPersonfromWhomAcquired>
<ns2:isGiftTransaction>N</ns2:isGiftTransaction>
<ns2:amountOfSecuritiesAcquired>25412</ns2:amountOfSecuritiesAcquired>
<ns2:paymentDate>10/01/2026</ns2:paymentDate>
<ns2:natureOfPayment>Compensation</ns2:natureOfPayment>
</ns2:securitiesToBeSold>
<ns2:nothingToReportFlagOnSecuritiesSoldInPast3Months>Y</ns2:nothingToReportFlagOnSecuritiesSoldInPast3Months>
<ns2:remarks>The sales of shares set forth herein are made in connection with a selling plan dated 5/21/2026, that is intended to comply with Rule 10b5-1(c).</ns2:remarks>
<ns2:noticeSignature>
<ns2:noticeDate>10/02/2026</ns2:noticeDate>
<ns2:planAdoptionDates><ns2:planAdoptionDate>05/21/2026</ns2:planAdoptionDate></ns2:planAdoptionDates>
<ns2:signature>Goldman Sachs &amp; Co. LLC on behalf of John Ternus</ns2:signature>
</ns2:noticeSignature>
</ns2:formData>
</ns2:edgarSubmission>"#;

    /// The past-three-months block, from Jennifer Newstead's notice on Apple,
    /// accession 0001950047-26-009738 (filed 22 September 2026).
    const PAST_SALE: &str = r#"<securitiesSoldInPast3Months>
<sellerDetails><name>10b5-1 Sales for JENNIFER G NEWSTEAD</name></sellerDetails>
<securitiesClassTitle>Common</securitiesClassTitle>
<saleDate>09/15/2026</saleDate>
<amountOfSecuritiesSold>1438</amountOfSecuritiesSold>
<grossProceeds>474813.22</grossProceeds>
</securitiesSoldInPast3Months>"#;

    #[test]
    fn a_notice_is_read_through_prefixed_elements() {
        let v = parse(TERNUS).unwrap();
        assert_eq!(v["issuer"]["cik"], "0000320193");
        assert_eq!(v["seller"]["name"], "John Ternus");
        assert_eq!(v["seller"]["relationship"], json!(["Officer"]));
        let sale = &v["proposed_sales"][0];
        assert_eq!(sale["shares"], json!(25_412));
        assert_eq!(sale["aggregate_market_value"], json!(8_479_730.28));
        assert_eq!(sale["approximate_sale_date"], "2026-10-02");
        assert_eq!(sale["broker"], "Goldman Sachs & Co. LLC");
        assert_eq!(v["acquired"][0]["acquired_from"], "Issuer");
        assert_eq!(v["acquired"][0]["gift"], json!(false));
        assert_eq!(v["rule_10b5_1_plan_adopted"], json!(["2026-05-21"]));
        assert_eq!(v["sold_past_3_months"], json!([]));
        assert!(v["remarks"].as_str().unwrap().contains("10b5-1"));
    }

    #[test]
    fn past_sales_are_listed() {
        let xml = TERNUS.replace(
            "<ns2:nothingToReportFlagOnSecuritiesSoldInPast3Months>Y</ns2:nothingToReportFlagOnSecuritiesSoldInPast3Months>",
            PAST_SALE,
        );
        let v = parse(&xml).unwrap();
        let sold = &v["sold_past_3_months"][0];
        assert_eq!(sold["seller"], "10b5-1 Sales for JENNIFER G NEWSTEAD");
        assert_eq!(sold["sale_date"], "2026-09-15");
        assert_eq!(sold["shares"], json!(1_438));
        assert_eq!(sold["gross_proceeds"], json!(474_813.22));
    }

    #[test]
    fn a_document_without_an_issuer_is_refused() {
        assert!(parse("<edgarSubmission><formData/></edgarSubmission>").is_err());
    }
}
