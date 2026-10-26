# Finance Agent Benchmark: 2026-10-26

Model `claude-opus-5-5`, sec-mcp 0.6.2 (commit `6dc0e15`), only sec-mcp tools, cutoff 2025-05-16. Dataset: Vals AI Finance Agent Benchmark, public 50 (CC BY 4.0), sha256 `bb33b8dbf21c…`.

| correct | partial | wrong | no answer |
|---|---|---|---|
| 0 (0%) | 1 | 2 | 1 |

95% Wilson interval on correct: 0–49%. Class-balanced accuracy (each category weighted equally, the paper's metric): 0%.

Tool calls: 30 (7.5 per question). Cost: $0.58. Period mismatches flagged: 0.

Cutoff audit: 22 accession citations; after the cutoff: 0; not in the session's tool results: 0.

## By category

| Category | Correct |
|---|---|
| Market Analysis | 0/1 |
| Numerical Reasoning | 0/1 |
| Qualitative Retrieval | 0/1 |
| Quantitative Retrieval | 0/1 |

## Per question

| id | category | grade | calls | note |
|---|---|---|---|---|
| 0 | Market Analysis | partial | 9 | The answer says the 3 Jan 2025 8-K exhibits carry the presidential order that blocked the deal, which meets the executive-order criterion. It does not mention the $7.3B unsolicited offer, U.S. Steel's rejection over strategic fit and undervaluation, or the late-2023 timing. Its suggestion that U.S. Steel later pushed for the deal is a hedged inference about later events. It does not directly deny the earlier rejection, so I did not count it as a contradiction. |
| 14 | Numerical Reasoning | wrong | 5 | The COGS of $14,060M matches. However, the answer gives average inventory as $2,148M and turnover as about 6.55x, which conflicts with the expected $2,168 and 6.49. It mentions 6.49 only as an alternative calculation using year-end inventory, while its headline answer is 6.55. That counts as a contradiction. |
| 28 | Qualitative Retrieval | no_answer | 7 | The answer says the values were not found. It names some KPI terms but gives no FY 2024 figures, so it meets no criterion. |
| 36 | Quantitative Retrieval | wrong | 9 | The answer's headline figure is about $4.79 billion, which it got by adding up yearly restructuring charges. That rounds to $4.8B, not the expected $4.7B. The answer also says it could not confirm WBD's disclosed cumulative figure. It never states $4.7B, so the correctness criterion is not met, and the $4.79B figure conflicts with the expected answer. |
