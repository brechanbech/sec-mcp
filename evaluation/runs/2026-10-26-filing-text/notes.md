# After `sec_filing_text`, 26 October 2026

Same 50 questions, model (Opus 5.5), cutoff (2025-05-16), instructions and
grader as the [baseline](../2026-10-26-baseline/notes.md). sec-mcp at commit
`cd25a58`: 0.7.0 plus `sec_filing_text`.

| | correct | partial | wrong | no answer | class-balanced |
|---|---|---|---|---|---|
| Baseline (0.6.2) | 11 (22%) | 7 | 9 | 23 | 21.6% |
| With `sec_filing_text` | **31 (62%)** | 8 | 11 | **0** | **52.9%** |

The 95% Wilson interval on 31/50 is 48–74%. There were 277 tool calls (5.5 per
question, down from 6.3) and the run cost $5.68 against $5.22. The cutoff audit
found 98 citations, none after the cutoff and none unverified.
`sec_filing_text` made 151 of the 277 calls.

## Paired: 20 fixed, none broken

| Baseline → now | Questions |
|---|---|
| correct → correct | 11: 3, 6, 8, 18, 19, 23, 25, 30, 34, 40, 44 |
| no answer → correct | 13: 2, 4, 12, 16, 20, 27, 28, 29, 31, 33, 36, 43, 46 |
| partial → correct | 4: 17, 35, 38, 39 |
| wrong → correct | 3: 21, 22, 26 |
| no answer → partial | 4: 1, 24, 47, 48 |
| no answer → wrong | 6: 0, 7, 10, 11, 37, 49 |
| wrong → partial | 1: 5 |
| partial → partial | 3: 15, 42, 45 |
| wrong → wrong | 5: 9, 13, 14, 32, 41 |

Every answer that was correct stayed correct. The biggest gains are where the
text was the whole problem:

| Category | Before | After |
|---|---|---|
| Qualitative Retrieval | 1/9 | 8/9 |
| Quantitative Retrieval | 1/9 | 7/9 |
| Beat or Miss | 1/7 | 3/7 |

## The wrong answers

None traces to a tool failure. They are choices of method or period, and the
grader holds the rubric's own:

- **Questions 9 and 14** are the expected answers flagged in the baseline notes
  as questionable: a two-year span with a three-year exponent, and year-end
  inventory labelled as average.
- **Question 37 (Lyft):** the agent took the guidance midpoint from the margin
  range, while the rubric derives it from the dollar range. It reports a 29 bp
  beat against the expected 26.1 bp.
- **Question 49 (AMD):** the agent used a different four-quarter window. Its
  three overlapping quarters match the expected figures exactly.
- **Question 10 (Airbnb shares):** the agent took share counts from the 10-K
  cover page date, while the rubric uses a different date.
- **Question 11 (TSMC):** the agent built the model from Q2 guidance, while the
  rubric uses Q1.
- **Question 7 (TKO):** the agent added a disclosed purchase-price adjustment to
  the expected $3.25B.
- **Question 13 (Zillow):** the agent used operating cash flow including
  discontinued operations, as in the baseline.
- **Question 32 (Boeing):** a different tax-rate assumption, as in the baseline.
- **Question 41 (Coca-Cola peers):** the agent's payout ratios differ from the
  rubric's for KDP and PEP.
- **Question 0 (US Steel):** the agent covers the executive order and the
  timing, but not the expected answer's $7.3B offer.

## Reading the number

62% raw and 52.9% class-balanced on the public 50, with this model and this
grader. Vals AI's paper reports o3 at 46.8% class-balanced on all 537 questions,
with web search and an EDGAR tool, in 2025. That is a different set of
questions, a different grader and an older model, so it says how hard the
questions are, not where sec-mcp ranks. The comparison that isolates the tool
is the one above: same model, same questions, same grader, 20 fixed and none
broken.
