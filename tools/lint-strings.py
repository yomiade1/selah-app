#!/usr/bin/env python3
"""UI string lint (gate stage 8, DESIGN-SYSTEM §7, ADR-010).

1. Every `content/i18n/<lang>/*.ftl` message has a kebab-case id that is unique in its language.
2. English strings use plain words: no banned words and no banned synonyms of product terms
   (`content/terms.yaml`).
3. `.axaml` files hold no literal user-visible text: Text, Content, Header, Title, Watermark,
   ToolTip.Tip and AutomationProperties.Name must be bindings or resources.

Exit 1 on any violation, printed as GitHub annotations.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
MESSAGE = re.compile(r"^(?P<id>-?[A-Za-z][A-Za-z0-9_-]*)\s*=\s*(?P<value>.*)$")
KEBAB = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
PLACEABLE = re.compile(r"\{[^}]*\}")
VISIBLE_ATTRIBUTES = re.compile(
    r'\b(?P<attr>Text|Content|Header|Title|Watermark|PlaceholderText|ToolTip\.Tip|AutomationProperties\.Name)="(?P<value>[^"]*)"')


def load_terms(path):
    """The subset of YAML that content/terms.yaml uses: lists, and one level of nested lists."""
    terms, section, term = {}, None, None
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split(" #", 1)[0].rstrip() if not raw.lstrip().startswith("#") else ""
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        text = line.strip()
        if indent == 0 and text.endswith(":"):
            section, term = text[:-1], None
            terms[section] = []
        elif text.startswith("- ") and section:
            value = text[2:].strip().strip('"')
            if term is not None and indent >= 4:
                terms[section][-1][1].append(value)
            else:
                terms[section].append(value)
        elif text.endswith(":") and section:
            term = text[:-1]
            if not isinstance(terms[section], list) or terms[section] and not isinstance(terms[section][0], tuple):
                terms[section] = []
            terms[section].append((term, []))
    return terms


def phrase_pattern(phrase):
    escaped = re.escape(phrase)
    start = r"(?<![\w])" if phrase[0].isalnum() else ""
    end = r"(?![\w])" if phrase[-1].isalnum() else ""
    return re.compile(start + escaped + end, re.IGNORECASE)


def annotate(path, line, message):
    print(f"::error file={path.relative_to(ROOT).as_posix()},line={line}::{message}")


def lint_ftl(terms):
    problems = 0
    banned = [(w, phrase_pattern(w)) for w in terms.get("banned_words", [])]
    synonyms = [(term, s, phrase_pattern(s)) for term, words in terms.get("product_terms", []) for s in words]
    for language_dir in sorted((ROOT / "content" / "i18n").iterdir()):
        if not language_dir.is_dir():
            continue
        seen = {}
        for path in sorted(language_dir.glob("*.ftl")):
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                match = MESSAGE.match(line)
                if not match:
                    continue
                message_id = match["id"]
                if not KEBAB.match(message_id.lstrip("-")):
                    annotate(path, number, f"message id '{message_id}' must be kebab-case")
                    problems += 1
                if message_id in seen:
                    annotate(path, number, f"'{message_id}' is also defined in {seen[message_id]}")
                    problems += 1
                seen[message_id] = path.name
                if language_dir.name != "en":
                    continue
                text = PLACEABLE.sub(" ", match["value"])
                for word, pattern in banned:
                    if pattern.search(text):
                        annotate(path, number, f"'{message_id}' uses '{word}'; see the word list in KNOWLEDGE-BASE §3.2")
                        problems += 1
                for term, synonym, pattern in synonyms:
                    if pattern.search(text):
                        annotate(path, number, f"'{message_id}' says '{synonym}'; Selah's word is '{term}'")
                        problems += 1
    return problems


def lint_axaml():
    problems = 0
    for path in sorted((ROOT / "app").rglob("*.axaml")):
        if any(part in {"bin", "obj"} for part in path.parts) or path.name.endswith(".g.axaml"):
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if line.strip().startswith("<!--"):
                continue
            for match in VISIBLE_ATTRIBUTES.finditer(line):
                value = match["value"].strip()
                if value and not value.startswith("{") and re.search(r"[A-Za-z]{2,}", value):
                    annotate(path, number, f"literal text in {match['attr']}=\"{value}\"; put it in content/i18n/en/*.ftl")
                    problems += 1
    return problems


def main():
    terms = load_terms(ROOT / "content" / "terms.yaml")
    if not terms.get("banned_words") or not terms.get("product_terms"):
        print("::error file=content/terms.yaml::banned_words and product_terms are required")
        return 1
    problems = lint_ftl(terms) + lint_axaml()
    if problems:
        print(f"lint-strings: {problems} problem(s)")
        return 1
    print("lint-strings: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
