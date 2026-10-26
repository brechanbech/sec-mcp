# Evaluation: Finance Agent Benchmark

Measures what sec-mcp lets a model answer, on questions nobody here wrote. The
plan and its reasoning are in [`docs/roadmap.md`](../docs/roadmap.md), under
"Measuring: the Finance Agent Benchmark".

## Dataset

The public 50 questions of the
[Vals AI Finance Agent Benchmark](https://huggingface.co/datasets/vals-ai/finance_agent_benchmark),
© Vals AI, licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
`data/finance_agent_benchmark.csv` is that file unmodified;
`data/questions.jsonl` is the same rows as JSON lines with an `id` (0-based row
order) added. The paper is [arXiv 2508.00828](https://arxiv.org/abs/2508.00828).

## Method

- **Answering** (`answer.sh`): one headless `claude -p` session per question.
  Its only tools are sec-mcp's: no built-in tools, so no fetch, search or shell;
  no other MCP server; no user or project settings, so no hooks; and an empty
  working directory, so no CLAUDE.md. The system prompt
  (`prompts/answer-system.md`) sets the point in time to 16 May 2025, the
  dataset's publication date, requires every figure to come from a tool result,
  and allows "Not found with these tools".
- **Grading** (`grade.sh`): a separate session with no tools grades each answer
  against the dataset's own rubric (`prompts/grade-system.md`). Each
  `correctness` criterion is met or not, the `contradiction` criterion is
  checked, and one grade is given: correct, partial, wrong or no answer. The
  grader is not told which system wrote the answer.
- **Summary** (`summarize.sh`): raw and class-balanced accuracy, the 95% Wilson
  interval, tool calls, cost, and a cutoff audit. The audit checks every
  accession number an answer cites against the filing date EDGAR returned in
  that session's tool results, not against the answer's own claims.

## Running

```zsh
evaluation/run.sh <run-name>            # all 50, three at a time
evaluation/run.sh <run-name> 9 14 19    # selected questions
evaluation/summarize.sh evaluation/runs/<run-name>
```

`run.sh` resumes: it skips questions already answered or graded. `MODEL`
overrides the model (default `claude-opus-5-5`), and `SEC_MCP_BIN` the server
binary (default `~/.cargo/bin/sec-mcp`, so install the build under test first).
sec-mcp must already have a contact email configured. Each run directory keeps
the full transcripts (`answers/*.jsonl`), the answers (`answers/*.md`), the
grades (`grades/*.json`) and `meta.json` (model, commit, sec-mcp version,
dataset hash, cutoff).

## Known limits

- **n = 50.** On 50 questions, the 95% interval around a 90% score is 79–96%.
- **The grader is a language model**, the same model that answered, and an
  answer citing accession numbers is recognisable, so "not told" is weaker than
  "could not tell".
- **Some expected answers are questionable.** Question 9 expects a 14.56%
  three-year revenue CAGR for Palantir, which is 2022–2024 revenue (a two-year
  span) raised to a one-third power. Question 0 describes a rejected $7.3
  billion offer where the filings show an agreed $55.00-a-share deal.
  edgar-audit-mcp's write-up flagged both. Grades follow the rubric anyway, and
  the report names these questions rather than adjusting their scores.
- **The cutoff is an instruction, not an enforced parameter.** The audit is
  what checks it.
