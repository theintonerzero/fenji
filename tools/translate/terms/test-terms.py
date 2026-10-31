#!/usr/bin/env python3
"""Tests for tools/translate/terms/plan_terms.py and assemble_terms.py: the glossary translated
afresh (retranslation spec, Amendment A). Stdlib only: python3 -I tools/translate/terms/test-terms.py"""
import importlib.util
import json
import pathlib
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


plan = load("plan_terms")

PERSON = "person: pinyin, family name first"
PLACE = "place: by meaning"


def term(zh, en="", kind="name", note=PERSON):
    return {"zh": zh, "en": en, "kind": kind, "variants": [], "note": note}


def line(fp, zh, en=None, env="", tight=False):
    return {"fp": fp, "env": env, "zh": zh, "en": en, "uses": [], "area": "other", "tight": tight}


# 天墉城 has a line of its own (its main-table fingerprint, from sha256sum), tight, and three
# longer lines hold it; 天墉 has none of its own.
TIANYONG_CITY_FP = "5c432b041b2ff23ce786e717f7df13c5"
LINES = [
    line(TIANYONG_CITY_FP, "天墉城", "Tianyong City", tight=True),
    line("01" * 16, "前往天墉城", "Go to Tianyong City"),
    line("02" * 16, "天墉城的弟子来了", "A Tianyong City disciple comes"),
    line("03" * 16, "天墉城外有一片竹林，风过时沙沙作响", "Outside Tianyong City is a bamboo grove"),
]
BULK = {TIANYONG_CITY_FP: {"budget": 14, "context": "maps.name, row 7", "tight": True}}


def lines_of(batches):
    return [l for b in batches for l in b["lines"]]


class Fingerprint(unittest.TestCase):
    def test_a_main_table_line_is_keyed_by_its_source(self):
        self.assertEqual(plan.fingerprint("剑"), "391aa9b897c33d688abd92e5c4af3d12")

    def test_a_context_line_is_keyed_by_its_env_a_nul_and_its_source(self):
        self.assertEqual(plan.fingerprint("剑", "noun"), "e4210820293fb08f7895783f0099939c")


class Rounds(unittest.TestCase):
    def test_an_entry_comes_a_round_after_every_entry_it_holds(self):
        self.assertEqual(
            plan.rounds(["天墉城弟子", "天墉", "剑", "天墉城"]),
            {"天墉": 0, "剑": 0, "天墉城": 1, "天墉城弟子": 2},
        )


class PlanRound(unittest.TestCase):
    def test_an_entry_with_a_line_of_its_own_takes_that_line_and_its_budget(self):
        batches, synthetic, _ = plan.plan_round(
            [term("天墉城", "Tianyong City", note=PLACE)], LINES, BULK, {}, 0
        )
        (bl,) = lines_of(batches)
        self.assertEqual(bl["fp"], TIANYONG_CITY_FP)
        self.assertEqual((bl["tight"], bl["budget"], bl["en_old"]), (True, 14, "Tianyong City"))
        self.assertEqual(synthetic, [])

    def test_an_entry_without_a_line_of_its_own_gets_one_in_the_term_context(self):
        lines = LINES[1:]
        batches, synthetic, entries = plan.plan_round(
            [term("天墉城", "Tianyong City", note=PLACE)], lines, {}, {}, 0
        )
        (bl,) = lines_of(batches)
        self.assertEqual(bl["fp"], "8e84c1be18cc747f34621c912734a8fd")
        self.assertEqual((bl["tight"], bl["budget"], bl["en_old"]), (False, None, "Tianyong City"))
        self.assertEqual(
            synthetic,
            [line("8e84c1be18cc747f34621c912734a8fd", "天墉城", "Tianyong City", env="term")],
        )
        self.assertEqual(entries, {"8e84c1be18cc747f34621c912734a8fd": "天墉城"})

    def test_the_context_says_what_it_is_how_many_lines_hold_it_and_shows_three(self):
        batches, _, _ = plan.plan_round(
            [term("天墉城", "Tianyong City", note=PLACE)], LINES, BULK, {}, 0
        )
        (bl,) = lines_of(batches)
        self.assertIn(PLACE, bl["context"])
        self.assertIn("4 lines hold it", bl["context"])
        self.assertIn("maps.name, row 7", bl["context"])
        for zh in ("前往天墉城", "天墉城的弟子来了", "天墉城外有一片竹林，风过时沙沙作响"):
            self.assertIn(zh, bl["context"])

    def test_a_round_plans_only_its_own_entries(self):
        glossary = [term("天墉", note=PLACE), term("天墉城", note=PLACE)]
        batches, _, _ = plan.plan_round(glossary, LINES, BULK, {}, 0)
        self.assertEqual([l["zh"] for l in lines_of(batches)], ["天墉"])

    def test_a_later_round_carries_the_held_entries_new_english(self):
        glossary = [term("天墉", "Tianyong (old)", note=PLACE), term("天墉城", note=PLACE)]
        batches, _, _ = plan.plan_round(glossary, LINES, BULK, {"天墉": "Tianyong"}, 1)
        (b,) = batches
        self.assertEqual(
            b["glossary"],
            [{"zh": "天墉", "en": "Tianyong", "kind": "name", "variants": [], "note": PLACE}],
        )

    def test_a_later_round_refuses_while_a_held_entry_has_no_english(self):
        glossary = [term("天墉", note=PLACE), term("天墉城", note=PLACE)]
        with self.assertRaisesRegex(ValueError, "天墉"):
            plan.plan_round(glossary, LINES, BULK, {}, 1)

    def test_batches_are_named_by_round_and_number(self):
        glossary = [term(zh) for zh in ("李一", "王二", "张三")]
        batches, _, _ = plan.plan_round(glossary, LINES, {}, {}, 0, max_lines=2)
        self.assertEqual([b["id"] for b in batches], ["terms-r0-00001", "terms-r0-00002"])

    def test_a_family_stays_in_one_batch_and_no_batch_passes_the_most_lines(self):
        glossary = [term(zh) for zh in ("王一", "欧阳甲", "欧阳乙", "李丙", "李丁")]
        batches, _, _ = plan.plan_round(glossary, LINES, {}, {}, 0, max_lines=3)
        sets = [{l["zh"] for l in b["lines"]} for b in batches]
        self.assertTrue(any({"欧阳甲", "欧阳乙"} <= s for s in sets), sets)
        self.assertTrue(all(len(b["lines"]) <= 3 for b in batches), sets)
        self.assertEqual(sorted(l["zh"] for l in lines_of(batches)), sorted(t["zh"] for t in glossary))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")


def read_jsonl(path):
    return [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines()]


A, B, C = "0a" * 16, "0b" * 16, "0c" * 16


class FinalEnglish(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.terms = pathlib.Path(self._tmp.name)
        write(self.terms / "batches" / "terms-r0-00001.json", {"id": "terms-r0-00001", "lines": [], "glossary": []})

    def tearDown(self):
        self._tmp.cleanup()

    def test_settled_beats_the_reviews_reading_which_beats_the_answers(self):
        write(self.terms / "answers" / "terms-r0-00001.json",
              {"lines": [{"fp": A, "en": "a1"}, {"fp": B, "en": "b1"}, {"fp": C, "en": "c1"}]})
        write(self.terms / "reviews" / "terms-r0-00001.json", {"lines": [
            {"fp": A, "keep": True, "en": None, "reason": None},
            {"fp": B, "keep": False, "en": "b2", "reason": "closer to the Chinese"},
            {"fp": C, "keep": False, "en": "c2", "reason": "closer to the Chinese"},
        ]})
        write(self.terms / "settled" / "terms-r0-00001.json",
              {"lines": [{"fp": C, "en": "c3", "reason": "the second, shortened"}]})
        self.assertEqual(plan.final_english(self.terms), {A: "a1", B: "b2", C: "c3"})

    def test_a_batch_not_yet_answered_gives_no_english(self):
        self.assertEqual(plan.final_english(self.terms), {})


class Main(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self._tmp.name)
        self.work, self.terms = root / "retranslation", root / "retranslation-terms"
        write(self.work / "glossary.json",
              {"terms": [term("天墉", note=PLACE), term("天墉城", "Tianyong City", note=PLACE)]})
        self.work.joinpath("lines.jsonl").write_text(
            "".join(json.dumps(l, ensure_ascii=False) + "\n" for l in LINES), encoding="utf-8")
        write(self.work / "rules.json", {"budget_percent": 130})
        write(self.work / "batches" / "interface-00001.json", {"id": "interface-00001", "glossary": [], "lines": [
            {"fp": TIANYONG_CITY_FP, "zh": "天墉城", "en_old": "Tianyong City",
             "context": "maps.name, row 7", "tight": True, "budget": 14}]})

    def tearDown(self):
        self._tmp.cleanup()

    def run_main(self, round_no):
        plan.main(["plan_terms.py", str(self.work), str(self.terms), str(round_no)])

    def test_round_zero_writes_its_batches_every_line_the_driver_needs_and_the_rules(self):
        self.run_main(0)
        (b,) = [json.loads(p.read_text()) for p in sorted((self.terms / "batches").glob("*.json"))]
        self.assertEqual((b["id"], [l["zh"] for l in b["lines"]]), ("terms-r0-00001", ["天墉"]))
        lines = read_jsonl(self.terms / "lines.jsonl")
        self.assertEqual(lines[: len(LINES)], LINES)
        self.assertEqual([(l["zh"], l["env"]) for l in lines[len(LINES):]], [("天墉", "term")])
        self.assertEqual(json.loads((self.terms / "rules.json").read_text()), {"budget_percent": 130})
        self.assertEqual(sorted(json.loads((self.terms / "entries.json").read_text()).values()), ["天墉", "天墉城"])

    def test_the_next_round_takes_the_last_ones_settled_english(self):
        self.run_main(0)
        (b,) = [json.loads(p.read_text()) for p in sorted((self.terms / "batches").glob("*.json"))]
        write(self.terms / "answers" / "terms-r0-00001.json", {"lines": [{"fp": b["lines"][0]["fp"], "en": "Tianyong"}]})
        self.run_main(1)
        r1 = json.loads((self.terms / "batches" / "terms-r1-00001.json").read_text())
        self.assertEqual(([l["zh"] for l in r1["lines"]], r1["glossary"][0]["en"]), (["天墉城"], "Tianyong"))
        self.assertEqual((r1["lines"][0]["fp"], r1["lines"][0]["budget"]), (TIANYONG_CITY_FP, 14))

    def test_a_round_already_planned_is_never_planned_over(self):
        self.run_main(0)
        with self.assertRaises(SystemExit):
            self.run_main(0)

    def test_it_asks_for_the_bulks_batches_first(self):
        for p in (self.work / "batches").glob("*.json"):
            p.unlink()
        with self.assertRaisesRegex(SystemExit, "translate batch"):
            self.run_main(0)


assemble = load("assemble_terms")
CREATURE = "creature: by meaning"


def assembled(zh, en, note=PERSON, kind="name"):
    new, missing = assemble.assemble([term(zh, kind=kind, note=note)], {A: zh}, {A: en})
    assert missing == [], missing
    return new[0]


class Assemble(unittest.TestCase):
    def test_each_entry_takes_its_settled_english_and_keeps_its_kind(self):
        self.assertEqual(
            assembled("天墉城", "Tianyong Citadel", note=PLACE),
            {"zh": "天墉城", "en": "Tianyong Citadel", "kind": "name", "variants": [], "note": "place"},
        )

    def test_the_note_is_rebuilt_as_its_kind_and_naming_rule_without_chinese(self):
        # glossary.tsv leaves re/ for fenji-content, so `translate glossary export` refuses Chinese.
        for note, want in [
            ("class: 司命 is the deity who governs fate; keep that sense", "class"),
            ("person: pinyin, family name first (柯慕青)", "person: pinyin, family name first"),
            ("person, an epithet: by meaning (龟贤人)", "person, an epithet"),
            ("stat: the genre's standard term", "stat: the genre's standard term"),
            ("a note with no kind of its own", ""),
        ]:
            with self.subTest(note=note):
                self.assertEqual(assembled("司命", "Arbiter of Fate", note=note)["note"], want)

    def test_a_person_in_pinyin_may_go_by_the_given_name(self):
        self.assertEqual(assembled("柯慕青", "Ke Muqing")["variants"], ["Muqing"])
        self.assertEqual(assembled("百里屠苏", "Baili Tusu")["variants"], ["Tusu"])

    def test_an_epithet_has_no_given_name(self):
        self.assertEqual(assembled("龟贤人", "Turtle Sage", note="person, an epithet: by meaning")["variants"], [])

    def test_a_creature_gets_the_plural_its_name_doesnt_hold(self):
        for en, want in [
            ("Shadow Wolf", ["Shadow Wolves"]),
            ("Grave Fairy", ["Grave Fairies"]),
            ("Iron Swordsman", ["Iron Swordsmen"]),
            ("Bone Knife", ["Bone Knives"]),
            ("Mountain Goat", []),
            ("Old Fox", []),
        ]:
            with self.subTest(en=en):
                self.assertEqual(assembled("狼", en, note=CREATURE)["variants"], want)

    def test_a_word_gets_no_variants(self):
        self.assertEqual(assembled("狼", "wolf", note=CREATURE, kind="word")["variants"], [])

    def test_an_entry_without_english_yet_is_listed(self):
        glossary = [term("天墉", note=PLACE), term("天墉城", note=PLACE)]
        _, missing = assemble.assemble(glossary, {A: "天墉", B: "天墉城"}, {B: "Tianyong Citadel"})
        self.assertEqual(missing, ["天墉"])


class AssembleMain(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self._tmp.name)
        self.work, self.terms = root / "retranslation", root / "retranslation-terms"
        write(self.work / "glossary.json", {"terms": [term("柯慕青"), term("天墉城", note=PLACE)]})
        write(self.terms / "entries.json", {A: "柯慕青", B: "天墉城"})
        write(self.terms / "batches" / "terms-r0-00001.json", {"id": "terms-r0-00001", "lines": [], "glossary": []})

    def tearDown(self):
        self._tmp.cleanup()

    def run_main(self):
        assemble.main(["assemble_terms.py", str(self.work), str(self.terms)])

    def test_it_writes_the_next_glossary_once_every_entry_has_english(self):
        write(self.terms / "answers" / "terms-r0-00001.json",
              {"lines": [{"fp": A, "en": "Ke Muqing"}, {"fp": B, "en": "Tianyong Citadel"}]})
        self.run_main()
        got = json.loads((self.work / "glossary.next.json").read_text(encoding="utf-8"))
        self.assertEqual([(t["zh"], t["en"], t["variants"]) for t in got["terms"]],
                         [("柯慕青", "Ke Muqing", ["Muqing"]), ("天墉城", "Tianyong Citadel", [])])

    def test_it_writes_nothing_while_an_entry_lacks_english(self):
        write(self.terms / "answers" / "terms-r0-00001.json", {"lines": [{"fp": A, "en": "Ke Muqing"}]})
        with self.assertRaisesRegex(SystemExit, "天墉城"):
            self.run_main()
        self.assertFalse((self.work / "glossary.next.json").exists())


if __name__ == "__main__":
    unittest.main()
