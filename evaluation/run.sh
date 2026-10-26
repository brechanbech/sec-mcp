#!/bin/zsh
# Run the public Finance Agent Benchmark questions through sec-mcp, then grade.
#
#   run.sh <run-name> [question-id ...]
#
# Answers three questions at a time: each session's EDGAR calls are sequential,
# so three sessions stay well inside EDGAR's 10 requests per second. Re-running
# skips questions that already have an answer or a grade, so an interrupted run
# resumes where it stopped.
set -eu

here=${0:A:h}
name=$1; shift
run=$here/runs/$name
ids=(${@:-$(jq -r .id "$here/data/questions.jsonl")})
bin=${SEC_MCP_BIN:-$HOME/.cargo/bin/sec-mcp}

mkdir -p "$run"
[[ -f $run/mcp.json ]] || print -r -- "{\"mcpServers\":{\"sec\":{\"command\":\"$bin\",\"args\":[]}}}" > "$run/mcp.json"
[[ -f $run/meta.json ]] || jq -n \
  --arg model "${MODEL:-claude-opus-5-5}" \
  --arg started "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg commit "$(git -C "$here" rev-parse --short HEAD)" \
  --arg sec_mcp "$(cd "$here/.." && cargo pkgid | sed 's/.*[#@]//')" \
  --arg dataset_sha256 "$(shasum -a 256 "$here/data/finance_agent_benchmark.csv" | cut -d' ' -f1)" \
  --arg cutoff 2025-05-16 \
  '$ARGS.named' > "$run/meta.json"

todo=()
for id in $ids; do
  [[ -s $run/answers/$(printf %02d $id).md ]] || todo+=$id
done
print "answering ${#todo} question(s)"
print -l -- $todo | xargs -P 3 -I{} "$here/answer.sh" {} "$run"

todo=()
for id in $ids; do
  [[ -s $run/grades/$(printf %02d $id).json ]] || todo+=$id
done
print "grading ${#todo} answer(s)"
print -l -- $todo | xargs -P 3 -I{} "$here/grade.sh" {} "$run"
