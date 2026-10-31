#!/usr/bin/env python3
"""The glossary translated afresh (retranslation spec, Amendment A, RA3): one round's batches.

Each glossary entry is one line, through the same driver, checks and review as the bulk: the
line the game shows it alone in, where there is one, so its budget is that line's; else a line
of its own in the `term` context. An entry comes in the round after every entry it holds, and
its batch's glossary carries those entries' new English, so the checks keep a name the same
inside every compound. Entries go in batches by kind and family: a family's people, a region's
places.

usage: plan_terms.py WORK TERMS ROUND
  WORK   the bulk's work folder (re/retranslation): glossary.json, lines.jsonl, rules.json, and
         batches/ as `translate batch` plans them, for the lines' budgets and contexts
  TERMS  the entries' work folder (re/retranslation-terms), for `translate --out TERMS drive`
  ROUND  0, then each next round once the last one's batches are settled
"""
import hashlib
import json
import shutil
import sys
from collections import defaultdict
from pathlib import Path

ENV = "term"
MAX_LINES = 60
EXAMPLES = 3
WHATS = ["class", "sect", "title", "place", "person", "creature", "skill", "item", "stat", "system", "other"]
# Two-character family names, which a person's family takes whole.
SURNAMES = {
    "欧阳", "司马", "上官", "东方", "南宫", "慕容", "公孙", "诸葛", "百里", "独孤", "令狐", "皇甫",
    "尉迟", "长孙", "宇文", "端木", "夏侯", "轩辕", "西门", "司徒", "太史", "钟离", "澹台", "公冶",
    "宗政", "濮阳", "淳于", "单于", "申屠", "闻人", "赫连", "呼延", "万俟", "拓跋", "北堂", "第五",
}


def fingerprint(zh, env=""):
    """fenji_pack::fingerprint_in: the first 16 bytes of the SHA-256 of the source, or of
    `<env>\\0<source>` in a context, as 32 hex digits."""
    source = zh.encode() if not env else env.encode() + b"\0" + zh.encode()
    return hashlib.sha256(source).hexdigest()[:32]


def holds(zhs):
    """Each entry's held entries: the others its Chinese contains."""
    every = set(zhs)
    return {
        z: {z[i:j] for i in range(len(z)) for j in range(i + 1, len(z) + 1)} & every - {z}
        for z in every
    }


def rounds(zhs):
    """Each entry's round: 0 for one that holds no other, else one after the last it holds."""
    held = holds(zhs)
    done = {}

    def of(z):
        if z not in done:
            done[z] = 1 + max(map(of, held[z])) if held[z] else 0
        return done[z]

    return {z: of(z) for z in held}


def what(t):
    """What an entry names, from the head of its note: `person`, `person, an epithet`."""
    head = t["note"].split(":", 1)[0].split(",", 1)[0].strip()
    return head if head in WHATS else "other"


def family(t):
    """The family an entry is batched with: a person's family name, a place's first two
    characters, else its kind alone."""
    kind, zh = what(t), t["zh"]
    if kind == "person":
        return zh[:2] if zh[:2] in SURNAMES else zh[:1]
    return zh[:2] if kind == "place" else ""


def own_lines(glossary, lines):
    """The line each entry has alone, from the main table where it has one."""
    by_zh = {t["zh"] for t in glossary}
    own = {}
    for l in lines:
        if l["zh"] in by_zh and (l["zh"] not in own or (l["env"] == "" and own[l["zh"]]["env"] != "")):
            own[l["zh"]] = l
    return own


def synthetic_line(t):
    """An entry's line of its own, for an entry the game never shows alone."""
    return {
        "fp": fingerprint(t["zh"], ENV),
        "env": ENV,
        "zh": t["zh"],
        "en": t["en"] or None,
        "uses": [],
        "area": "other",
        "tight": False,
    }


def uses(glossary, lines):
    """How many lines hold each entry, and up to EXAMPLES of the shortest others that do."""
    every = {t["zh"] for t in glossary}
    longest = max((len(z) for z in every), default=0)
    count = defaultdict(int)
    examples = defaultdict(list)
    for l in lines:
        zh = l["zh"]
        found = {
            zh[i:j]
            for i in range(len(zh))
            for j in range(i + 1, min(len(zh), i + longest) + 1)
            if zh[i:j] in every
        }
        for z in found:
            count[z] += 1
            if zh != z:
                examples[z].append(zh)
    shortest = {z: sorted(ex, key=lambda s: (len(s), s))[:EXAMPLES] for z, ex in examples.items()}
    return count, shortest


def context(t, count, examples, own_context):
    parts = [f"{t['note']}."]
    held_by = f"{count} lines hold it"
    if examples:
        held_by += ", such as: " + " / ".join(examples)
    parts.append(held_by + ".")
    if own_context:
        parts.append(f"Its own line: {own_context}.")
    return " ".join(parts)


def plan_round(glossary, lines, bulk, english, round_no, max_lines=MAX_LINES):
    """Round `round_no`'s batches, the lines of its own its entries need, and its entries by
    fingerprint. `bulk` holds the bulk's batch lines' budgets and contexts by fingerprint;
    `english`, the new English of the entries earlier rounds settled."""
    by_zh = {t["zh"]: t for t in glossary}
    held = holds(by_zh)
    round_of = rounds(by_zh)
    chosen = sorted(
        (t for t in glossary if round_of[t["zh"]] == round_no),
        key=lambda t: (WHATS.index(what(t)), family(t), t["zh"]),
    )
    missing = sorted({h for t in chosen for h in held[t["zh"]] if h not in english})
    if missing:
        raise ValueError(f"round {round_no}: no new English yet for {', '.join(missing)}")
    own = own_lines(glossary, lines)
    count, examples = uses(glossary, lines)

    synthetic, entries, families = [], {}, []
    for t in chosen:
        l = own.get(t["zh"])
        if l:
            info = bulk.get(l["fp"], {})
            bl = {
                "fp": l["fp"],
                "zh": t["zh"],
                "en_old": l["en"],
                "context": context(t, count[t["zh"]], examples.get(t["zh"], []), info.get("context")),
                "tight": info.get("tight", l["tight"]),
                "budget": info.get("budget"),
            }
        else:
            s = synthetic_line(t)
            synthetic.append(s)
            bl = {
                "fp": s["fp"],
                "zh": t["zh"],
                "en_old": s["en"],
                "context": context(t, count[t["zh"]], examples.get(t["zh"], []), None),
                "tight": False,
                "budget": None,
            }
        entries[bl["fp"]] = t["zh"]
        key = (what(t), family(t))
        if families and families[-1][0] == key:
            families[-1][1].append(bl)
        else:
            families.append((key, [bl]))

    packs, current = [], []
    for _, members in families:
        if current and len(current) + len(members) > max_lines:
            packs.append(current)
            current = []
        while len(members) > max_lines:
            packs.append(members[:max_lines])
            members = members[max_lines:]
        current.extend(members)
    if current:
        packs.append(current)

    batches = []
    for k, pack in enumerate(packs, 1):
        terms = sorted({h for bl in pack for h in held[bl["zh"]]})
        batches.append(
            {
                "id": f"terms-r{round_no}-{k:05}",
                "lines": pack,
                "glossary": [
                    {
                        "zh": h,
                        "en": english[h],
                        "kind": by_zh[h]["kind"],
                        "variants": [],
                        "note": by_zh[h]["note"],
                    }
                    for h in terms
                ],
            }
        )
    return batches, synthetic, entries


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def final_english(terms):
    """Each line's English from the driver's files, as `translate assemble` takes it: settled,
    else the review's, else the answer's."""
    every = {}
    for p in sorted((terms / "batches").glob("*.json")):
        bid = read_json(p)["id"]
        answer = terms / "answers" / f"{bid}.json"
        if not answer.exists():
            continue
        en = {l["fp"]: l["en"] for l in read_json(answer)["lines"]}
        review = terms / "reviews" / f"{bid}.json"
        if review.exists():
            en.update({l["fp"]: l["en"] for l in read_json(review)["lines"] if not l["keep"] and l.get("en")})
        settled = terms / "settled" / f"{bid}.json"
        if settled.exists():
            en.update({l["fp"]: l["en"] for l in read_json(settled)["lines"]})
        for fp, e in en.items():
            if fp in every:
                raise ValueError(f"{fp} is in two batches' answers")
            every[fp] = e
    return every


def main(argv):
    if len(argv) != 4:
        sys.exit(__doc__)
    work, terms, round_no = Path(argv[1]), Path(argv[2]), int(argv[3])
    glossary = read_json(work / "glossary.json")["terms"]
    lines = [json.loads(l) for l in (work / "lines.jsonl").read_text(encoding="utf-8").splitlines()]
    bulk = {
        bl["fp"]: {"budget": bl["budget"], "context": bl["context"], "tight": bl["tight"]}
        for p in sorted((work / "batches").glob("*.json"))
        for bl in read_json(p)["lines"]
    }
    if not bulk:
        sys.exit(f"{work / 'batches'}: plan the bulk's batches first, with translate batch: they hold the lines' budgets")
    planned = sorted((terms / "batches").glob(f"terms-r{round_no}-*.json"))
    if planned:
        sys.exit(f"round {round_no} is planned already: {len(planned)} batches in {terms / 'batches'}")
    english = {}
    if round_no > 0:
        entries = read_json(terms / "entries.json")
        english = {entries[fp]: en for fp, en in final_english(terms).items() if fp in entries}
    try:
        batches, _, _ = plan_round(glossary, lines, bulk, english, round_no)
    except ValueError as e:
        sys.exit(str(e))

    # Every entry's line, whatever its round, so the driver finds each round's lines.
    own = own_lines(glossary, lines)
    synthetic = [synthetic_line(t) for t in glossary if t["zh"] not in own]
    entries = {(own[t["zh"]]["fp"] if t["zh"] in own else fingerprint(t["zh"], ENV)): t["zh"] for t in glossary}
    (terms / "batches").mkdir(parents=True, exist_ok=True)
    new = terms / "lines.jsonl.new"
    with open(new, "w", encoding="utf-8") as f:
        for l in lines + synthetic:
            f.write(json.dumps(l, ensure_ascii=False) + "\n")
    new.replace(terms / "lines.jsonl")
    shutil.copyfile(work / "rules.json", terms / "rules.json")
    write_json(terms / "entries.json", entries)
    for b in batches:
        write_json(terms / "batches" / f"{b['id']}.json", b)
    print(f"round {round_no}: {sum(len(b['lines']) for b in batches)} entries in {len(batches)} batches")


if __name__ == "__main__":
    main(sys.argv)
