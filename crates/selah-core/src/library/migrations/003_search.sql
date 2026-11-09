CREATE VIRTUAL TABLE songs_trigram USING fts5(
    title,
    authors,
    ccli_number,
    tags,
    lyrics,
    tokenize = 'trigram'
);

CREATE VIRTUAL TABLE media_trigram USING fts5(
    name,
    tags,
    tokenize = 'trigram'
);

CREATE TABLE search_synonyms (
    term TEXT NOT NULL,
    synonym TEXT NOT NULL,
    PRIMARY KEY(term, synonym)
) STRICT;

CREATE INDEX search_synonyms_synonym ON search_synonyms(synonym);

CREATE TABLE search_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
