#!/bin/sh
# A stand-in for `claude -p` in tools/translate's driver tests (retranslation plan, Task 7). It
# reads the prompt on stdin and answers in the CLI's JSON shape, as FAKE_CLAUDE_MODE says:
#   ok         a valid answer for every fingerprint in the prompt's lines;
#   short      drops one line;
#   wrong-fp   answers one line under a fingerprint the batch doesn't hold;
#   junk       prints something that isn't JSON;
#   limit      the CLI's usage-limit result, in the docs' words with no reset time, once, then
#              as ok;
#   overloaded the API's 529, once, then as ok;
#   bad-token  drops a {0}, once, then as ok.
# Its English is the source's ASCII (its markup) after a word, so it passes the checks. A review
# replaces the lines whose source holds REVISE. It appends one line per run to FAKE_CLAUDE_LOG
# (its job, how many runs were going, its working directory, its arguments in brackets, the
# schema's as [schema], and the fingerprints in its prompt), and writes each prompt to a file of
# its own in FAKE_CLAUDE_LOG.prompts/. FAKE_CLAUDE_SLEEP, in seconds, makes a run last.
set -u
log="${FAKE_CLAUDE_LOG:?}"
mode="${FAKE_CLAUDE_MODE:-ok}"
running="$log.running"
mkdir -p "$running"
: > "$running/$$"
trap 'rm -f "$running/$$"' EXIT
going=$(ls "$running" | wc -l | tr -d ' ')

args=""
schema=""
prev=""
for a in "$@"; do
  if [ "$prev" = "--json-schema" ]; then
    schema="$a"
    args="$args [schema]"
  else
    args="$args [$a]"
  fi
  prev="$a"
done
prompt=$(cat)
mkdir -p "$log.prompts"
printf '%s\n' "$prompt" > "$(mktemp "$log.prompts/XXXXXXXX")"

job=translate
case "$schema" in
  *'"keep"'*) job=review ;;
  *'"reason"'*) job=settle ;;
esac

# Misbehaving once: the first run that can. mkdir is atomic, so two runs can't both be first.
once() {
  mkdir "$log.once" 2>/dev/null
}
answer() {
  printf '%s\n' "$prompt" | LC_ALL=C tr -d '\200-\377' | awk -v job="$job" -v mode="$1" '
  BEGIN { n = 0 }
  /^ *"fp": "/ { fp = $0; sub(/^ *"fp": "/, "", fp); sub(/".*/, "", fp); next }
  /^ *"zh": "/ && fp != "" {
    zh = $0; sub(/^ *"zh": "/, "", zh); sub(/",? *$/, "", zh)
    fps[n] = fp; zhs[n] = zh; n++; fp = ""; next
  }
  END {
    last = (mode == "short") ? n - 1 : n
    printf "{\"lines\":["
    for (i = 0; i < last; i++) {
      f = fps[i]; z = zhs[i]
      if (mode == "wrong-fp" && i == 0) f = "ffffffffffffffffffffffffffffffff"
      if (i > 0) printf ","
      if (job == "translate") {
        en = "Text " z
        if (mode == "bad-token-now") sub(/\{0\}/, "", en)
        printf "{\"fp\":\"%s\",\"en\":\"%s\"}", f, en
      } else if (job == "review") {
        if (z ~ /REVISE/) printf "{\"fp\":\"%s\",\"keep\":false,\"en\":\"Better %s\",\"reason\":\"Livelier.\"}", f, z
        else printf "{\"fp\":\"%s\",\"keep\":true,\"en\":null,\"reason\":null}", f
      } else {
        printf "{\"fp\":\"%s\",\"en\":\"Settled %s\",\"reason\":\"Closer to the source.\"}", f, z
      }
    }
    printf "]}"
  }'
}
case "$mode" in
  limit) once && mode=limit-now || mode=ok ;;
  overloaded) once && mode=overloaded-now || mode=ok ;;
  # Only an answer that holds a {0} can drop one; the style guide's examples hold some too.
  bad-token)
    if answer ok | grep -q '{0}' && once; then mode=bad-token-now; else mode=ok; fi ;;
esac
answer=$(answer "$mode")
fps=$(printf '%s\n' "$prompt" | sed -n 's/^ *"fp": "\([0-9a-f]*\)".*/\1/p' | tr '\n' ',')
printf '%s %s going=%s cwd=%s args=%s fps=%s\n' "$job" "$mode" "$going" "$(pwd)" "$args" "$fps" >> "$log"

sleep "${FAKE_CLAUDE_SLEEP:-0}"
case "$mode" in
  junk) echo "Sorry, I can't help with that." ;;
  limit-now)
    printf '{"type":"result","subtype":"success","is_error":true,"result":"You'"'"'ve hit your session limit","session_id":"fake"}\n'
    exit 1 ;;
  overloaded-now)
    printf '{"type":"result","subtype":"success","is_error":true,"api_error_status":529,"result":"API Error: Repeated 529 Overloaded errors","session_id":"fake"}\n'
    exit 1 ;;
  *)
    printf '{"type":"result","subtype":"success","is_error":false,"api_error_status":null,"result":"","structured_output":%s,"session_id":"fake","num_turns":2,"duration_ms":1000,"total_cost_usd":0.5,"usage":{"input_tokens":10,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":20}}\n' "$answer" ;;
esac
