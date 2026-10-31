#!/usr/bin/env python3
"""The glossary translated afresh (retranslation spec, Amendment A, RA4): the next glossary.

Each entry takes its settled English from the entries' work folder, with its kind, and a note
and variants rebuilt from the new English. The note is the entry's kind and naming rule, never
Chinese: `translate glossary export` takes the notes out of re/ to fenji-content. The checks find
a name anywhere in a line's English, so a plural that holds the name needs no variant; what's
rebuilt is a person's given name, where the name is in pinyin, and a creature's plural where it
doesn't hold the name (Wolves). What these don't cover is reviewed in a session (RA5).

usage: assemble_terms.py WORK TERMS
  writes WORK/glossary.next.json once every entry of WORK/glossary.json has its English in TERMS
"""
import importlib.util
import sys
from pathlib import Path

_spec = importlib.util.spec_from_file_location("plan_terms", Path(__file__).with_name("plan_terms.py"))
assert _spec and _spec.loader
plan = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(plan)

# Each kind's note in the next glossary: its kind and naming rule, and never Chinese, since
# `translate glossary export` takes the notes out of re/ to fenji-content.
NOTES = {
    "person": "person: pinyin, family name first",
    "class": "class",
    "sect": "sect",
    "title": "title",
    "place": "place",
    "creature": "creature",
    "skill": "skill",
    "item": "a recurring word in items' names",
    "stat": "stat: the genre's standard term",
    "system": "a term of the game's systems",
    "other": "",
}
# Plurals that don't hold their singular, by ending.
IRREGULAR = {"man": "men", "person": "people", "mouse": "mice", "louse": "lice", "goose": "geese", "tooth": "teeth", "foot": "feet"}
# Words in -f that take -fs.
TAKE_S = {"chief", "roof", "belief", "chef", "reef", "proof", "brief", "gulf", "serf", "turf", "surf"}


def plural(word):
    """A word's plural where it doesn't hold the word itself, else None."""
    lower = word.lower()
    for one, many in IRREGULAR.items():
        if lower.endswith(one):
            head = word[: len(word) - len(one)]
            return head + (many.capitalize() if word[len(head)].isupper() else many)
    if lower.endswith("fe"):
        return word[:-2] + "ves"
    if lower.endswith("f") and not lower.endswith("ff") and lower not in TAKE_S:
        return word[:-1] + "ves"
    if len(word) > 1 and lower.endswith("y") and lower[-2] not in "aeiou":
        return word[:-1] + "ies"
    return None


def variants(t, en):
    """The forms a line may write a name in besides its English."""
    if t["kind"] != "name":
        return []
    kind, note, words = plan.what(t), t["note"].lower(), en.split()
    out = []
    if kind == "person" and "pinyin" in note and "epithet" not in note and len(words) >= 2:
        out.append(" ".join(words[1:]))
    if kind == "creature" and words:
        many = plural(words[-1])
        if many:
            out.append(" ".join(words[:-1] + [many]))
    return [v for v in dict.fromkeys(out) if v != en]


def note(t):
    """An entry's note rebuilt from its draft's: its kind, and for a person how the name goes."""
    kind = plan.what(t)
    if kind == "person" and "epithet" in t["note"].lower():
        return "person, an epithet"
    return NOTES[kind]


def assemble(glossary, entries, english):
    """The next glossary's terms, and the entries with no English yet. `entries` maps each
    entry's fingerprint to its Chinese; `english`, each fingerprint to its settled English."""
    zh_en = {zh: english[fp] for fp, zh in entries.items() if fp in english}
    new, missing = [], []
    for t in glossary:
        en = zh_en.get(t["zh"])
        if en is None:
            missing.append(t["zh"])
            continue
        new.append({"zh": t["zh"], "en": en, "kind": t["kind"], "variants": variants(t, en), "note": note(t)})
    return new, missing


def main(argv):
    if len(argv) != 3:
        sys.exit(__doc__)
    work, terms = Path(argv[1]), Path(argv[2])
    glossary = plan.read_json(work / "glossary.json")["terms"]
    new, missing = assemble(glossary, plan.read_json(terms / "entries.json"), plan.final_english(terms))
    if missing:
        more = " …" if len(missing) > 20 else ""
        sys.exit(f"{len(missing)} entries have no English yet: {', '.join(missing[:20])}{more}")
    plan.write_json(work / "glossary.next.json", {"terms": new})
    print(f"{len(new)} entries, {sum(1 for t in new if t['variants'])} with variants: {work / 'glossary.next.json'}")


if __name__ == "__main__":
    main(sys.argv)
