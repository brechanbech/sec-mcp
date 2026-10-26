#!/bin/zsh
# Grade one answer against the benchmark's expected answer and rubric.
#
#   grade.sh <question-id> <run-dir>
#
# Reads <run-dir>/answers/<id>.md and writes <run-dir>/grades/<id>.json. The
# grader gets no tools and is not told which system wrote the answer.
set -eu

id=$1
run=${2:A}
here=${0:A:h}
model=${MODEL:-claude-opus-5-5}

n=$(printf %02d "$id")
mkdir -p "$run/grades"
row=$(jq -c --argjson id "$id" 'select(.id == $id)' "$here/data/questions.jsonl")

prompt=$(jq -rn --argjson r "$row" --rawfile a "$run/answers/$n.md" '
  "## Question\n\n\($r.question)\n\n## Expected answer\n\n\($r.answer)\n\n"
  + "## Rubric\n\n\($r.rubric)\n\n## Answer to grade\n\n\($a)"')

schema='{
  "type": "object",
  "properties": {
    "criteria": {"type": "array", "items": {"type": "object",
      "properties": {"criterion": {"type": "string"}, "met": {"type": "boolean"}},
      "required": ["criterion", "met"]}},
    "contradiction": {"type": "boolean"},
    "period_mismatch": {"type": "boolean"},
    "grade": {"enum": ["correct", "partial", "wrong", "no_answer"]},
    "reason": {"type": "string"}
  },
  "required": ["criteria", "contradiction", "period_mismatch", "grade", "reason"]
}'

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
claude -p "$prompt" \
  --model "$model" \
  --system-prompt "$(<"$here/prompts/grade-system.md")" \
  --tools "" \
  --strict-mcp-config \
  --setting-sources local \
  --no-session-persistence \
  --output-format json \
  --json-schema "$schema" \
  | jq --argjson id "$id" '.structured_output + {id: $id}' > "$run/grades/$n.json"
