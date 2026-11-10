# Bundled Bible translations and provenance (H-06)

Per [BUILD-PLAN.md](../../BUILD-PLAN.md) human input **H-06**, Selah currently bundles **only** two public-domain English translations (**KJV** and **WEB**) across the 66 canonical books. Additional translations require confirmed redistribution rights (**M2-A10**).

## 1. King James Version (`KJV`)

- **Display name:** King James Version
- **Language:** `en` (English)
- **Canonical scope:** 66 books, 31,102 verses
- **Legal status:** Public domain in the United States and internationally (1769 Oxford/Blayney standard text). Note: Crown Letters Patent restrictions apply only to commercial printing in England/Wales/Northern Ireland under royal prerogative.
- **Upstream source:** `scrollmapper/bible_databases`, commit `59b18c5af5845728c275d5b7178ad0d024172b1f`, file `formats/csv/KJV.csv` (repository MIT License)
- **Upstream raw SHA-256 (`KJV.csv`):** `a1051b4395bee78d01e239984425f3919563db5caee6fc162d4ddc7faff7eaa8`
- **Bundled asset (`content/bibles/kjv.tsv.gz`):** 1,308,847 bytes
- **Bundled SHA-256 (`kjv.tsv.gz`):** `d47745bced43d6a23bd15846fcb5ecfff4e0b942f76cbb4e6ed3c503c0f796d7`

## 2. World English Bible (`WEB`)

- **Display name:** World English Bible
- **Language:** `en` (English)
- **Canonical scope:** 66 books, 31,098 verses (canonical Protestant books from eBible WEB master text; four Textus Receptus bracketed verses absent from modern eBible WEB JSON)
- **Legal status:** Public domain (explicitly dedicated to the public domain by Rainbow Missions / eBible.org; the title "World English Bible" applies only to faithful copies of the text)
- **Upstream source:** `TehShrike/world-english-bible`, commit `68669ba3be9719ae4d1135b19d9e0b6587b7c356` (derived from `ebible.org/web/`, canonical 66 books JSON)
- **Upstream tarball SHA-256:** `d28f7b8de1b2390c2da61f59bd1ccfc8310489f58d525e2fe467d4dadc63bcff`
- **Bundled asset (`content/bibles/web.tsv.gz`):** 1,293,616 bytes
- **Bundled SHA-256 (`web.tsv.gz`):** `a3a811b6dc9c2fbd7d725af2ee762ea250999f3bf8773532cdd718a0766310d1`

## Format of `*.tsv.gz`

Deterministic gzip stream (`mtime = 0`) containing UTF-8 tab-separated records ordered by `(book_num, chapter, verse)`:

```text
<book_num 1..66>\t<chapter>\t<verse>\t<verse_text>\n
```

`selah-core` verifies SHA-256 / UTF-8 integrity and builds indexed per-translation SQLite databases (`KJV.db`, `WEB.db`) via atomic write-and-rename into `library/bibles/` (SEC-15).
