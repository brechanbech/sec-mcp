#!/bin/zsh
# Answer one benchmark question with Claude, allowed only sec-mcp's tools.
#
#   answer.sh <question-id> <run-dir>
#
# Writes <run-dir>/answers/<id>.jsonl (the full stream-json transcript, tool
# calls included) and <run-dir>/answers/<id>.md (the final answer text).
#
# Isolation: no built-in tools (no fetch, search or shell), no MCP server but
# sec-mcp, no user or project settings (so no hooks), our own system prompt, and
# an empty working directory so no CLAUDE.md is discovered.
set -eu

id=$1
run=${2:A}
here=${0:A:h}
model=${MODEL:-claude-opus-5-5}

question=$(jq -r --argjson id "$id" 'select(.id == $id) | .question' "$here/data/questions.jsonl")
[[ -n $question ]] || { print -u2 "no question $id"; exit 1 }

mkdir -p "$run/answers"
out="$run/answers/$(printf %02d "$id")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A question that runs away is cut off rather than allowed to stall the run.
cd "$work"
timeout 900 claude -p "$question" \
  --model "$model" \
  --system-prompt "$(<"$here/prompts/answer-system.md")" \
  --tools "" \
  --strict-mcp-config --mcp-config "$run/mcp.json" \
  --allowedTools "mcp__sec" \
  --setting-sources local \
  --no-session-persistence \
  --output-format stream-json --verbose \
  > "$out.jsonl" || print -u2 "question $id: claude exited $?"

jq -r 'select(.type == "result") | .result // empty' "$out.jsonl" > "$out.md"
