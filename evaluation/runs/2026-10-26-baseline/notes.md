# Baseline, 26 October 2026: what it found

sec-mcp 0.6.2 with Opus 5.5, only sec-mcp tools, cutoff 2025-05-16. Scores are
in [`summary.md`](summary.md): **11 of 50 correct (22%, 95% interval
12.8–35.2%)**, 7 partial, 9 wrong, 23 no answer; class-balanced 21.6%. There
were 314 tool calls (6.3 per question) and the run cost $5.22. The cutoff audit
found 167 accession citations and none filed after the cutoff.

## Most failures are the missing filing text (roadmap item 2)

The 23 no-answers are almost all the agent saying, correctly, that the figure
sits in text it cannot read: guidance in earnings releases (Beat or Miss, 1/7),
KPIs and definitions in 10-K prose (Qualitative Retrieval 1/9, Quantitative
Retrieval 1/9), deal terms in 8-Ks. Full-text search finds *which* filing holds
a phrase but never returns the text around it, and the agent repeatedly used it
as a yes/no oracle for guessed phrases. The XBRL-shaped categories did better:
Numerical Reasoning 4/8, Adjustments 2/4.

## Two sec-mcp bugs, cheaper to fix than item 2

1. **Companies without a current ticker are unreachable.** Every tool is keyed
   by ticker, and the ticker list is SEC's *current* one. Four questions hit
   this: US Steel (`X`, taken private), Spirit Airlines (`SAVE`, `SAVEQ`),
   Warner Bros. Discovery (`WBD`, `DISCA`). In question 28 the agent tried
   passing Spirit's CIK as the ticker; that failed too. Question 14 recovered
   only by finding US Steel's rows in cross-company frames. Every tool should
   accept a CIK as well as a ticker. That matters beyond the benchmark: any
   point-in-time question meets delisted and renamed companies.
2. **Some responses are too large for the client.** Six results, five from
   `sec_company_facts` (94k–181k characters) and one from
   `sec_insider_transactions` (114k), exceeded Claude Code's tool-output limit.
   The client saved them to a file the agent had no tool to read, so the
   calls were wasted. `sec_company_facts` needs paging or a filter, as
   `sec_financial_concept` and `sec_list_tickers` got in 0.6.0, and
   `sec_insider_transactions` a tighter default.

Also seen: 27 calls returned a 404 for a concept the company does not report
(Palantir has no `Revenues`, only `RevenueFromContractWithCustomer…`). The
agent recovered each time, but the error could point at `sec_company_facts`
instead of being a bare 404.

## Answers graded wrong that are not tool failures

- **Question 13 (Zillow FCF)**: reported 2022 operating cash flow includes the
  Zillow Offers wind-down (discontinued operations). The expected 24.8% uses the
  continuing-operations concept, which the agent never tried.
- **Question 26 (Allstate debentures)**: the agent stated coupon and maturity
  figures that came from no tool result, against its instructions. A model
  failure, graded wrong.
- **Question 32 (Boeing refinancing)**: a different tax-rate assumption.

## Expected answers that look questionable

Grades follow the rubric regardless; these are named so the score can be read
correctly.

- **Question 9 (Palantir CAGR)**: expects 14.56%, which is two years of growth
  (2022–2024) raised to a one-third power. The agent's 22.9% for 2021–2024 is
  a correct three-year CAGR.
- **Question 14 (US Steel inventory turnover)**: expects 6.49 from an "average"
  inventory of $2,168M, which is the year-end figure alone. The agent's 6.55
  uses the true average of $2,128M and $2,168M, and it gave 6.49 as the
  year-end variant.
- **Question 5 (capex plans)**: the rubric's Meta range ($60–65B) is the
  January 2025 guidance. Meta raised it to $64–72B in its Q1 2025 release on
  30 April 2025, before the cutoff. The agent found that range by confirming the
  exact phrase in the release with full-text search.
- **Question 0 (US Steel)**: already flagged in edgar-audit-mcp's write-up.
  It describes a rejected $7.3B offer where the filings show an agreed $55.00
  per share deal.

Counting questions 9 and 14 as correct would make it 13 of 50 (26%). The
headline stays at 11.
