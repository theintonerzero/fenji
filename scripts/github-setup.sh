#!/usr/bin/env bash
# One-time (idempotent) GitHub setup for the Fenji repo: labels, milestones M0–M5, default branch `dev`.
# Requires an authenticated `gh` and a GitHub remote. Safe to re-run.
#
#   scripts/github-setup.sh            apply
#   scripts/github-setup.sh --dry-run  print what would be done
set -euo pipefail

DRY=0
[ "${1:-}" = "--dry-run" ] && DRY=1

run() {
  if [ "$DRY" -eq 1 ]; then echo "would run: $*"; else "$@"; fi
}

command -v gh >/dev/null || { echo "gh CLI not found" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "gh is not authenticated: run 'gh auth login'" >&2; exit 1; }
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "Repository: $REPO"

# name|color|description
LABELS="type:task|1d76db|Planned unit of work (one task, one PR)
type:bug|d73a4a|Something behaves incorrectly
type:re|5319e7|Reverse-engineering investigation
decision-gate|b60205|Work stopped; needs the owner's decision
status:blocked|e4e669|Waiting on another issue or a decision
area:proto|0e8a16|fenji-proto: framing, scramble, codec, handshake, payloads
area:data|0e8a16|fenji-data: archives, tables, content overlays
area:lua|0e8a16|fenji-lua: bytecode, decompiler, VM
area:auth|0e8a16|fenji-auth: invites, accounts, tokens
area:store|0e8a16|fenji-store: persistence
area:server|0e8a16|fenji-server: listeners, world, admin
area:launcher|0e8a16|launcher crate and Tauri app
area:tools|0e8a16|archive, luatool, stub
area:deploy|0e8a16|VPS deployment
area:docs|0e8a16|documentation
area:dev-env|0e8a16|bootstrap, CI, sandbox, tooling"

echo "$LABELS" | while IFS='|' read -r name color desc; do
  run gh label create "$name" --color "$color" --description "$desc" --force
done

# title|description
MILESTONES="M0 Dev environment + spike|Tooling ported, pinned build recorded, triage written up, sandbox runs the client non-root, one real-client run against the stub (release 0.1.0)
M1 Data layer|archive dumps items, skills, monsters, XP curve, maps; luatool reads the embedded Lua; Oodle-for-archives path decided (release 0.2.0)
M2 Transport + handshake|Stub completes the real client's gate handshake and decodes its first packets; Oodle Network path decided (release 0.3.0)
M3 Login + lobby|Invite accounts, launcher, ticket handoff, character list/create/delete in the real client (release 0.4.0)
M4 Social sandbox + VPS|Friends enter the starting zone, move, see each other, chat; admin commands; deployed (release 0.5.0)
M5 Grind loop (v1)|Monsters, AI, combat, XP, loot, inventory, persistence, live rates (release 1.0.0)"

existing=$(gh api "repos/$REPO/milestones?state=all&per_page=100" -q '.[].title')
echo "$MILESTONES" | while IFS='|' read -r title desc; do
  if printf '%s\n' "$existing" | grep -qxF "$title"; then
    echo "milestone exists: $title"
  else
    run gh api "repos/$REPO/milestones" -f title="$title" -f description="$desc" --silent
  fi
done

if git show-ref --verify --quiet refs/remotes/origin/dev; then
  run gh repo edit "$REPO" --default-branch dev --enable-squash-merge --enable-merge-commit --enable-rebase-merge=false --delete-branch-on-merge
else
  echo "origin/dev not pushed yet; push dev, then re-run to make it the default branch"
fi
