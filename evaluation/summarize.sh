#!/bin/zsh
# Summarise a graded run into <run-dir>/summary.md and <run-dir>/results.json.
#
#   summarize.sh <run-dir>
#
# The cutoff audit does not trust the answer's own dates. Every accession number
# an answer cites is looked up in that session's tool results, where EDGAR's
# filing date travels beside it (`accn`/`filed`, `accession`/`filed`,
# `accession_number`/`date`). A cited accession filed after the cutoff is a
# violation; one the tools never returned is reported as unverified.
set -eu

run=${1:A}
here=${0:A:h}
cutoff=$(jq -r .cutoff "$run/meta.json")

rows=()
for f in $run/grades/*.json; do
  n=${f:t:r}
  t=$run/answers/$n.jsonl
  # accession -> filing date, from every object in every tool result.
  dates=$(jq -s '
    [ .[] | select(.type == "user") | .message.content[]?
      | select(.type == "tool_result")
      | (.content | if type == "array" then map(.text // "") | join("") else . end)
      | (try fromjson catch null) | select(. != null)
      | .. | objects
      | { a: (.accn // .accession // .accession_number), d: (.filed // .date) }
      | select((.a | type) == "string" and (.d | type) == "string") ]
    | map({ (.a): .d }) | add // {}' "$t")
  rows+=$(jq -s -c --argjson dates "$dates" --arg cutoff "$cutoff" \
      --rawfile answer "$run/answers/$n.md" --slurpfile g "$f" '
    ($g[0]) as $g
    | ([ $answer | scan("\\b\\d{10}-\\d{2}-\\d{6}\\b") ] | unique) as $cited
    | {
        id: $g.id, grade: $g.grade, contradiction: $g.contradiction,
        period_mismatch: $g.period_mismatch, reason: $g.reason,
        tool_calls: [ .[] | select(.type == "assistant") | .message.content[]?
                      | select(.type == "tool_use") ] | length,
        cost_usd: ([ .[] | select(.type == "result") | .total_cost_usd ] | add // 0),
        cited: ($cited | length),
        after_cutoff: [ $cited[] | select(($dates[.] // "") > $cutoff) ],
        unverified: [ $cited[] | select($dates[.] == null) ]
      }' "$run/answers/$n.jsonl")
done

print -rl -- $rows | jq -s --slurpfile q "$here/data/questions.jsonl" '
  (map({ (.id | tostring): . }) | add) as $by
  | [ $q[] | . as $row | ($by[$row.id | tostring] // empty)
      + { type: $row.type, question: $row.question } ]
' > "$run/results.json"

jq -r --argjson meta "$(<"$run/meta.json")" '
  def pct: . * 1000 | round / 10;
  def wilson($k; $n): ($k / $n) as $p | 1.96 as $z
    | (1 + $z * $z / $n) as $d
    | (($p + $z * $z / (2 * $n)) / $d) as $c
    | ($z * (($p * (1 - $p) / $n + $z * $z / (4 * $n * $n)) | sqrt) / $d) as $h
    | "\(($c - $h) | pct)–\(($c + $h) | pct)%";
  length as $n
  | (map(select(.grade == "correct")) | length) as $k
  | (group_by(.type) | map({ type: .[0].type, n: length,
      correct: (map(select(.grade == "correct")) | length) })) as $types
  | "# Finance Agent Benchmark: \($meta.started[:10])\n",
    "Model `\($meta.model)`, sec-mcp \($meta.sec_mcp) (commit `\($meta.commit)`), only sec-mcp tools, cutoff \($meta.cutoff). Dataset: Vals AI Finance Agent Benchmark, public 50 (CC BY 4.0), sha256 `\($meta.dataset_sha256[:12])…`.\n",
    "| correct | partial | wrong | no answer |", "|---|---|---|---|",
    "| \($k) (\($k / $n | pct)%) | \(map(select(.grade == "partial")) | length) | \(map(select(.grade == "wrong")) | length) | \(map(select(.grade == "no_answer")) | length) |\n",
    "95% Wilson interval on correct: \(wilson($k; $n)). Class-balanced accuracy (each category weighted equally, the paper'"'"'s metric): \($types | map(.correct / .n) | add / length | pct)%.\n",
    "Tool calls: \(map(.tool_calls) | add) (\(map(.tool_calls) | add / $n * 10 | round / 10) per question). Cost: $\(map(.cost_usd) | add * 100 | round / 100). Period mismatches flagged: \(map(select(.period_mismatch)) | length).\n",
    "Cutoff audit: \(map(.cited) | add) accession citations; after the cutoff: \(map(.after_cutoff | length) | add); not in the session'"'"'s tool results: \(map(.unverified | length) | add).\n",
    "## By category\n", "| Category | Correct |", "|---|---|",
    ($types[] | "| \(.type) | \(.correct)/\(.n) |"),
    "\n## Per question\n", "| id | category | grade | calls | note |", "|---|---|---|---|---|",
    (.[] | "| \(.id) | \(.type) | \(.grade)\(if .period_mismatch then " (period)" else "" end) | \(.tool_calls) | \(.reason | gsub("\\|"; "/") | gsub("\n"; " ")) |")
' "$run/results.json" > "$run/summary.md"

print "wrote $run/summary.md"
