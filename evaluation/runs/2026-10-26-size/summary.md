# Finance Agent Benchmark: 2026-10-26

Model `claude-opus-5-5`, sec-mcp 0.6.2 (commit `0b8673f`), only sec-mcp tools, cutoff 2025-05-16. Dataset: Vals AI Finance Agent Benchmark, public 50 (CC BY 4.0), sha256 `bb33b8dbf21c…`.

| correct | partial | wrong | no answer |
|---|---|---|---|
| 2 (28.6%) | 0 | 0 | 5 |

95% Wilson interval on correct: 8.2–64.1%. Class-balanced accuracy (each category weighted equally, the paper's metric): 20.8%.

Tool calls: 47 (6.7 per question). Cost: $0.96. Period mismatches flagged: 0.

Cutoff audit: 16 accession citations; after the cutoff: 0; not in the session's tool results: 0.

## By category

| Category | Correct |
|---|---|
| Financial Modeling | 0/1 |
| Qualitative Retrieval | 1/2 |
| Quantitative Retrieval | 1/3 |
| Trends | 0/1 |

## Per question

| id | category | grade | calls | note |
|---|---|---|---|---|
| 1 | Trends | no_answer | 5 | The answer says it could not find the ARM figures and declines to estimate them. Instead it gives total revenue, which is a different metric and does not conflict with the expected ARM values. It meets no criterion. |
| 6 | Qualitative Retrieval | correct | 3 | The answer names Elinor (Ellie) Mertz as Airbnb's CFO, which matches the expected answer. Nothing in it conflicts with the expected answer. |
| 7 | Quantitative Retrieval | correct | 8 | The answer gives $3.25 billion as the value of the Endeavor asset deal, paid in TKO OpCo common units, and cites the closing 8-K and the Q1 2025 10-Q. It hedges by saying it couldn't confirm that this is the value measured at closing and that the closing value could differ. It never states a different figure, so there is no contradiction. The required figure is present, so the single correctness criterion is met. |
| 20 | Qualitative Retrieval | no_answer | 9 | The answer says it could not find what the filing says about vendor concentration. It does not state the single North America processing vendor or the backup-systems belief. |
| 27 | Quantitative Retrieval | no_answer | 7 | The answer says it could not find the total 2025 material cash requirements, so it does not give the expected $14,426,266,000. It does give one component, content obligations of $11.42B. That figure is labeled as a part of the total, not the total itself, so it does not conflict with the expected answer. No criterion is met. |
| 29 | Financial Modeling | no_answer | 8 | The answer says the figure was not found and gives no share count. It meets no criterion and doesn't contradict the expected answer. |
| 31 | Quantitative Retrieval | no_answer | 7 | The answer says outright that it could not find the purchase vs. refinance split, so it gives neither expected figure. Instead it reports total additions to loans held for sale, about $17.03B, and labels this as a different metric (a proxy) that is not the requested split. That figure doesn't conflict with either expected component, so I don't count it as a contradiction. Because the answer declines the question and meets no criterion, the grade is no_answer. |
