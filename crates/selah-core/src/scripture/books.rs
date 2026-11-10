//! Canonical 66-book table and 6-language (`en`, `es`, `pt`, `fr`, `ko`, `yo`) alias resolution.
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::library::fold_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookSpec {
    pub book_num: u32,
    /// Canonical 3-character USFM / Selah book code (`GEN`..`REV`).
    pub code: &'static str,
    /// OSIS XML book identifier (`Gen`..`Rev`).
    pub osis: &'static str,
    /// Standard English display name.
    pub name: &'static str,
    pub is_single_chapter: bool,
    pub en: &'static [&'static str],
    pub es: &'static [&'static str],
    pub pt: &'static [&'static str],
    pub fr: &'static [&'static str],
    pub ko: &'static [&'static str],
    pub yo: &'static [&'static str],
}

pub const BOOKS: [BookSpec; 66] = [
    BookSpec {
        book_num: 1,
        code: "GEN",
        osis: "Gen",
        name: "Genesis",
        is_single_chapter: false,
        en: &["genesis", "gen", "ge", "gn"],
        es: &["génesis", "genesis", "gén", "gen", "gn"],
        pt: &["gênesis", "genesis", "gên", "gen", "gn"],
        fr: &["genèse", "genese", "gen", "gn", "ge"],
        ko: &["창세기", "창"],
        yo: &["gẹnẹsisi", "jẹnẹsisi", "genesisi", "gẹn", "gen"],
    },
    BookSpec {
        book_num: 2,
        code: "EXO",
        osis: "Exod",
        name: "Exodus",
        is_single_chapter: false,
        en: &["exodus", "exod", "ex", "exo"],
        es: &["éxodo", "exodo", "éx", "ex", "exo"],
        pt: &["êxodo", "exodo", "êx", "ex", "exo"],
        fr: &["exode", "ex", "exo"],
        ko: &["출애굽기", "출"],
        yo: &["ẹ́ksódù", "eksodu", "ẹks", "eks"],
    },
    BookSpec {
        book_num: 3,
        code: "LEV",
        osis: "Lev",
        name: "Leviticus",
        is_single_chapter: false,
        en: &["leviticus", "lev", "le", "lv"],
        es: &["levítico", "levitico", "lv", "lev"],
        pt: &["levítico", "levitico", "lv", "lev"],
        fr: &["lévitique", "levitique", "lv", "lév", "lev"],
        ko: &["레위기", "레"],
        yo: &["léfítíkù", "lefitiku", "lef"],
    },
    BookSpec {
        book_num: 4,
        code: "NUM",
        osis: "Num",
        name: "Numbers",
        is_single_chapter: false,
        en: &["numbers", "num", "nu", "nm", "nb"],
        es: &["números", "numeros", "nm", "núm", "num"],
        pt: &["números", "numeros", "nm", "núm", "num"],
        fr: &["nombres", "nb", "nom", "nomb"],
        ko: &["민수기", "민"],
        yo: &["númérì", "numeri", "num"],
    },
    BookSpec {
        book_num: 5,
        code: "DEU",
        osis: "Deut",
        name: "Deuteronomy",
        is_single_chapter: false,
        en: &["deuteronomy", "deut", "deu", "dt"],
        es: &["deuteronomio", "dt", "deut", "deu"],
        pt: &["deuteronômio", "deuteronomio", "dt", "deut"],
        fr: &["deutéronome", "deuteronome", "dt", "deut"],
        ko: &["신명기", "신"],
        yo: &["diutarónómì", "deuteronomi", "deu"],
    },
    BookSpec {
        book_num: 6,
        code: "JOS",
        osis: "Josh",
        name: "Joshua",
        is_single_chapter: false,
        en: &["joshua", "josh", "jos", "jsh"],
        es: &["josué", "josue", "jos"],
        pt: &["josué", "josue", "js", "jos"],
        fr: &["josué", "josue", "jos"],
        ko: &["여호수아", "수"],
        yo: &["jóṣùà", "joṣua", "josua", "joṣ", "jos"],
    },
    BookSpec {
        book_num: 7,
        code: "JDG",
        osis: "Judg",
        name: "Judges",
        is_single_chapter: false,
        en: &["judges", "judg", "jdg", "jg", "jdgs"],
        es: &["jueces", "jue", "jc"],
        pt: &["juízes", "juizes", "jz", "jui"],
        fr: &["juges", "jg", "jug"],
        ko: &["사사기", "삿"],
        yo: &["onídàájọ́", "onidajọ", "onidajo", "oni"],
    },
    BookSpec {
        book_num: 8,
        code: "RUT",
        osis: "Ruth",
        name: "Ruth",
        is_single_chapter: false,
        en: &["ruth", "rut", "rth", "ru"],
        es: &["rut", "rt"],
        pt: &["rute", "rt", "rut"],
        fr: &["ruth", "rt", "ru"],
        ko: &["룻기", "룻"],
        yo: &["rútù", "rutu", "rut"],
    },
    BookSpec {
        book_num: 9,
        code: "1SA",
        osis: "1Sam",
        name: "1 Samuel",
        is_single_chapter: false,
        en: &[
            "1 samuel", "1 sam", "1 sa", "1 sm", "1s", "i samuel", "i sam", "i sa",
        ],
        es: &["1 samuel", "1 s", "1 sam", "1 sa", "i samuel"],
        pt: &["1 samuel", "1 sm", "1 sam", "i samuel"],
        fr: &["1 samuel", "1 s", "1 sam", "1 sa", "i samuel"],
        ko: &["사무엘상", "삼상"],
        yo: &["1 sámúẹ́lì", "1 samuẹli", "1 samueli", "1 sam"],
    },
    BookSpec {
        book_num: 10,
        code: "2SA",
        osis: "2Sam",
        name: "2 Samuel",
        is_single_chapter: false,
        en: &[
            "2 samuel",
            "2 sam",
            "2 sa",
            "2 sm",
            "2s",
            "ii samuel",
            "ii sam",
            "ii sa",
        ],
        es: &["2 samuel", "2 s", "2 sam", "2 sa", "ii samuel"],
        pt: &["2 samuel", "2 sm", "2 sam", "ii samuel"],
        fr: &["2 samuel", "2 s", "2 sam", "2 sa", "ii samuel"],
        ko: &["사무엘하", "삼하"],
        yo: &["2 sámúẹ́lì", "2 samuẹli", "2 samueli", "2 sam"],
    },
    BookSpec {
        book_num: 11,
        code: "1KI",
        osis: "1Kgs",
        name: "1 Kings",
        is_single_chapter: false,
        en: &[
            "1 kings", "1 kgs", "1 ki", "1 k", "1kgs", "i kings", "i kgs",
        ],
        es: &["1 reyes", "1 r", "1 rey", "i reyes"],
        pt: &["1 reis", "1 rs", "1 re", "i reis"],
        fr: &["1 rois", "1 r", "1 roi", "i rois"],
        ko: &["열왕기상", "왕상"],
        yo: &["1 àwọn ọba", "1 awon oba", "1 ọba", "1 oba"],
    },
    BookSpec {
        book_num: 12,
        code: "2KI",
        osis: "2Kgs",
        name: "2 Kings",
        is_single_chapter: false,
        en: &[
            "2 kings", "2 kgs", "2 ki", "2 k", "2kgs", "ii kings", "ii kgs",
        ],
        es: &["2 reyes", "2 r", "2 rey", "ii reyes"],
        pt: &["2 reis", "2 rs", "2 re", "ii reis"],
        fr: &["2 rois", "2 r", "2 roi", "ii rois"],
        ko: &["열왕기하", "왕하"],
        yo: &["2 àwọn ọba", "2 awon oba", "2 ọba", "2 oba"],
    },
    BookSpec {
        book_num: 13,
        code: "1CH",
        osis: "1Chr",
        name: "1 Chronicles",
        is_single_chapter: false,
        en: &[
            "1 chronicles",
            "1 chron",
            "1 chr",
            "1 ch",
            "i chronicles",
            "i chr",
        ],
        es: &["1 crónicas", "1 cronicas", "1 cr", "1 crón", "i crónicas"],
        pt: &["1 crônicas", "1 cronicas", "1 cr", "1 crô", "i crônicas"],
        fr: &["1 chroniques", "1 ch", "1 chr", "i chroniques"],
        ko: &["역대상", "대상"],
        yo: &["1 króníkà", "1 kronika", "1 kro"],
    },
    BookSpec {
        book_num: 14,
        code: "2CH",
        osis: "2Chr",
        name: "2 Chronicles",
        is_single_chapter: false,
        en: &[
            "2 chronicles",
            "2 chron",
            "2 chr",
            "2 ch",
            "ii chronicles",
            "ii chr",
        ],
        es: &["2 crónicas", "2 cronicas", "2 cr", "2 crón", "ii crónicas"],
        pt: &["2 crônicas", "2 cronicas", "2 cr", "2 crô", "ii crônicas"],
        fr: &["2 chroniques", "2 ch", "2 chr", "ii chroniques"],
        ko: &["역대하", "대하"],
        yo: &["2 króníkà", "2 kronika", "2 kro"],
    },
    BookSpec {
        book_num: 15,
        code: "EZR",
        osis: "Ezra",
        name: "Ezra",
        is_single_chapter: false,
        en: &["ezra", "ezr"],
        es: &["esdras", "esd"],
        pt: &["esdras", "ed", "esd"],
        fr: &["esdras", "esd"],
        ko: &["에스라", "스"],
        yo: &["ẹ́srá", "ẹsra", "esra", "ẹsr"],
    },
    BookSpec {
        book_num: 16,
        code: "NEH",
        osis: "Neh",
        name: "Nehemiah",
        is_single_chapter: false,
        en: &["nehemiah", "neh", "ne"],
        es: &["nehemías", "nehemias", "neh", "ne"],
        pt: &["neemias", "ne", "nee"],
        fr: &["néhémie", "nehemie", "né", "ne", "néh", "neh"],
        ko: &["느헤미야", "느"],
        yo: &["nehemáyà", "nehemiah", "nehemia", "neh"],
    },
    BookSpec {
        book_num: 17,
        code: "EST",
        osis: "Esth",
        name: "Esther",
        is_single_chapter: false,
        en: &["esther", "esth", "est"],
        es: &["ester", "est"],
        pt: &["ester", "et", "est"],
        fr: &["esther", "est"],
        ko: &["에스더", "에"],
        yo: &["ẹ́stérì", "ẹstẹri", "esteri", "ẹst", "est"],
    },
    BookSpec {
        book_num: 18,
        code: "JOB",
        osis: "Job",
        name: "Job",
        is_single_chapter: false,
        en: &["job", "jb"],
        es: &["job", "jb"],
        pt: &["jó", "job"],
        fr: &["job", "jb"],
        ko: &["욥기", "욥"],
        yo: &["jóòbù", "jobu"],
    },
    BookSpec {
        book_num: 19,
        code: "PSA",
        osis: "Ps",
        name: "Psalms",
        is_single_chapter: false,
        en: &["psalms", "psalm", "psa", "ps", "pss", "psm"],
        es: &["salmos", "salmo", "sal", "sl"],
        pt: &["salmos", "salmo", "sl", "sal"],
        fr: &["psaumes", "psaume", "ps", "psa"],
        ko: &["시편", "시"],
        yo: &[
            "orin dáfídì",
            "orin dafidi",
            "sáàmù",
            "saamu",
            "orin",
            "dafidi",
        ],
    },
    BookSpec {
        book_num: 20,
        code: "PRO",
        osis: "Prov",
        name: "Proverbs",
        is_single_chapter: false,
        en: &["proverbs", "prov", "pro", "prv", "pr"],
        es: &["proverbios", "prov", "pr"],
        pt: &["provérbios", "proverbios", "pv", "pr", "prov"],
        fr: &["proverbes", "pr", "prov"],
        ko: &["잠언", "잠"],
        yo: &["òwe", "owe"],
    },
    BookSpec {
        book_num: 21,
        code: "ECC",
        osis: "Eccl",
        name: "Ecclesiastes",
        is_single_chapter: false,
        en: &["ecclesiastes", "eccl", "ecc", "ec", "qoh"],
        es: &["eclesiastés", "eclesiastes", "ec", "ecl"],
        pt: &["eclesiastes", "ec", "ecl"],
        fr: &["ecclésiaste", "ecclesiaste", "ec", "qo", "qoh"],
        ko: &["전도서", "전"],
        yo: &["oníwàásù", "oniwaasu", "oniw"],
    },
    BookSpec {
        book_num: 22,
        code: "SNG",
        osis: "Song",
        name: "Song of Solomon",
        is_single_chapter: false,
        en: &[
            "song of solomon",
            "song of songs",
            "song",
            "sos",
            "sng",
            "cant",
            "canticles",
            "ss",
        ],
        es: &["cantar de los cantares", "cantares", "cnt", "ct", "cant"],
        pt: &[
            "cânticos",
            "canticos",
            "cantares de salomão",
            "cantares",
            "ct",
            "cânt",
        ],
        fr: &["cantique des cantiques", "cantique", "ct", "ca", "cant"],
        ko: &["아가", "아"],
        yo: &[
            "orin sọlọmọni",
            "orin solomoni",
            "orin sọlọ́mọ́nì",
            "sọlọmọni",
        ],
    },
    BookSpec {
        book_num: 23,
        code: "ISA",
        osis: "Isa",
        name: "Isaiah",
        is_single_chapter: false,
        en: &["isaiah", "isa", "is"],
        es: &["isaías", "isaias", "is", "isa"],
        pt: &["isaías", "isaias", "is", "isa"],
        fr: &["ésaïe", "esaie", "és", "es", "is", "isaïe"],
        ko: &["이사야", "사"],
        yo: &["aisaya", "aísáyà", "isaiah", "ais"],
    },
    BookSpec {
        book_num: 24,
        code: "JER",
        osis: "Jer",
        name: "Jeremiah",
        is_single_chapter: false,
        en: &["jeremiah", "jer", "je", "jr"],
        es: &["jeremías", "jeremias", "jer", "jr"],
        pt: &["jeremias", "jr", "jer"],
        fr: &["jérémie", "jeremie", "jr", "jér", "jer"],
        ko: &["예레미야", "렘"],
        yo: &["jeremáyà", "jeremiah", "jeremia", "jer"],
    },
    BookSpec {
        book_num: 25,
        code: "LAM",
        osis: "Lam",
        name: "Lamentations",
        is_single_chapter: false,
        en: &["lamentations", "lam", "la"],
        es: &["lamentaciones", "lm", "lam"],
        pt: &[
            "lamentações",
            "lamentacoes",
            "lamentações de jeremias",
            "lm",
            "lam",
        ],
        fr: &["lamentations", "lm", "lam"],
        ko: &["예레미야애가", "애가", "애"],
        yo: &["ẹkún jeremáyà", "ẹkún jeremiah", "ekun jeremiah", "ekun"],
    },
    BookSpec {
        book_num: 26,
        code: "EZK",
        osis: "Ezek",
        name: "Ezekiel",
        is_single_chapter: false,
        en: &["ezekiel", "ezek", "ezk", "eze"],
        es: &["ezequiel", "ez", "ezq", "eze"],
        pt: &["ezequiel", "ez", "ezeq"],
        fr: &["ézéchiel", "ezechiel", "éz", "ez", "ézéch", "ezek"],
        ko: &["에스겔", "겔"],
        yo: &["esekíẹ́lì", "esekiẹli", "esekieli", "esek"],
    },
    BookSpec {
        book_num: 27,
        code: "DAN",
        osis: "Dan",
        name: "Daniel",
        is_single_chapter: false,
        en: &["daniel", "dan", "da", "dn"],
        es: &["daniel", "dn", "dan"],
        pt: &["daniel", "dn", "dan"],
        fr: &["daniel", "dn", "dan"],
        ko: &["다니엘", "단"],
        yo: &["daniẹ́lì", "daniẹli", "danieli", "dan"],
    },
    BookSpec {
        book_num: 28,
        code: "HOS",
        osis: "Hos",
        name: "Hosea",
        is_single_chapter: false,
        en: &["hosea", "hos", "ho"],
        es: &["oseas", "os"],
        pt: &["oseias", "oséias", "os"],
        fr: &["osée", "osee", "os"],
        ko: &["호세아", "호"],
        yo: &["hoséà", "hosea", "hos"],
    },
    BookSpec {
        book_num: 29,
        code: "JOL",
        osis: "Joel",
        name: "Joel",
        is_single_chapter: false,
        en: &["joel", "jol", "jl"],
        es: &["joel", "jl"],
        pt: &["joel", "jl"],
        fr: &["joël", "joel", "jl"],
        ko: &["요엘", "욜"],
        yo: &["joẹ́lì", "joẹli", "joeli"],
    },
    BookSpec {
        book_num: 30,
        code: "AMO",
        osis: "Amos",
        name: "Amos",
        is_single_chapter: false,
        en: &["amos", "amo", "am"],
        es: &["amós", "amos", "am"],
        pt: &["amós", "amos", "am"],
        fr: &["amos", "am"],
        ko: &["아모스", "암"],
        yo: &["ámọ́sì", "amọsi", "amosi", "amọ", "amo"],
    },
    BookSpec {
        book_num: 31,
        code: "OBA",
        osis: "Obad",
        name: "Obadiah",
        is_single_chapter: true,
        en: &["obadiah", "obad", "oba", "ob"],
        es: &["abdías", "abdias", "abd"],
        pt: &["obadias", "ob", "obd"],
        fr: &["abdias", "ab", "abd"],
        ko: &["오바댜", "옵"],
        yo: &["ọbadáyà", "ọbadiah", "obadiah", "ọba", "oba"],
    },
    BookSpec {
        book_num: 32,
        code: "JON",
        osis: "Jonah",
        name: "Jonah",
        is_single_chapter: false,
        en: &["jonah", "jon", "jnh"],
        es: &["jonás", "jonas", "jon"],
        pt: &["jonas", "jn", "jon"],
        fr: &["jonas", "jon"],
        ko: &["요나", "욘"],
        yo: &["jónà", "jona"],
    },
    BookSpec {
        book_num: 33,
        code: "MIC",
        osis: "Mic",
        name: "Micah",
        is_single_chapter: false,
        en: &["micah", "mic", "mi", "mc"],
        es: &["miqueas", "miq", "mi"],
        pt: &["miqueias", "miquéias", "mq", "miq"],
        fr: &["michée", "michee", "mi", "mic"],
        ko: &["미가", "미"],
        yo: &["míkà", "mika", "mik"],
    },
    BookSpec {
        book_num: 34,
        code: "NAM",
        osis: "Nah",
        name: "Nahum",
        is_single_chapter: false,
        en: &["nahum", "nah", "nam", "na"],
        es: &["nahúm", "nahum", "nah", "na"],
        pt: &["naum", "na"],
        fr: &["nahum", "na", "nah"],
        ko: &["나훔", "나"],
        yo: &["nahúmù", "nahumu", "nah"],
    },
    BookSpec {
        book_num: 35,
        code: "HAB",
        osis: "Hab",
        name: "Habakkuk",
        is_single_chapter: false,
        en: &["habakkuk", "hab"],
        es: &["habacuc", "hab"],
        pt: &["habacuque", "hc", "hab"],
        fr: &["habacuc", "habakuk", "ha", "hab"],
        ko: &["하박국", "합"],
        yo: &["habákúkù", "habakuku", "hab"],
    },
    BookSpec {
        book_num: 36,
        code: "ZEP",
        osis: "Zeph",
        name: "Zephaniah",
        is_single_chapter: false,
        en: &["zephaniah", "zeph", "zep", "zp"],
        es: &["sofonías", "sofonias", "sof"],
        pt: &["sofonias", "sf", "sof"],
        fr: &["sophonie", "so", "soph"],
        ko: &["스바냐", "습"],
        yo: &["sefánáyà", "sefaniah", "sef"],
    },
    BookSpec {
        book_num: 37,
        code: "HAG",
        osis: "Hag",
        name: "Haggai",
        is_single_chapter: false,
        en: &["haggai", "hag", "hg"],
        es: &["hageo", "hag", "hg"],
        pt: &["ageu", "ag"],
        fr: &["aggée", "aggee", "ag", "agg"],
        ko: &["학개", "학"],
        yo: &["hágáì", "hagai", "hag"],
    },
    BookSpec {
        book_num: 38,
        code: "ZEC",
        osis: "Zech",
        name: "Zechariah",
        is_single_chapter: false,
        en: &["zechariah", "zech", "zec", "zc"],
        es: &["zacarías", "zacarias", "zac", "zc"],
        pt: &["zacarias", "zc", "zac"],
        fr: &["zacharie", "za", "zac", "zach"],
        ko: &["스가랴", "슥"],
        yo: &["sekáráyà", "sekariah", "sek"],
    },
    BookSpec {
        book_num: 39,
        code: "MAL",
        osis: "Mal",
        name: "Malachi",
        is_single_chapter: false,
        en: &["malachi", "mal", "ml"],
        es: &["malaquías", "malaquias", "mal"],
        pt: &["malaquias", "ml", "mal"],
        fr: &["malachie", "ml", "mal"],
        ko: &["말라기", "말"],
        yo: &["málákì", "malaki", "mal"],
    },
    BookSpec {
        book_num: 40,
        code: "MAT",
        osis: "Matt",
        name: "Matthew",
        is_single_chapter: false,
        en: &["matthew", "matt", "mat", "mt"],
        es: &["mateo", "mt", "mat"],
        pt: &["mateus", "mt", "mat"],
        fr: &["matthieu", "mt", "mat", "matt"],
        ko: &["마태복음", "마태", "마"],
        yo: &["mátíù", "matiu", "mat"],
    },
    BookSpec {
        book_num: 41,
        code: "MRK",
        osis: "Mark",
        name: "Mark",
        is_single_chapter: false,
        en: &["mark", "mrk", "mk", "mr"],
        es: &["marcos", "mc", "mr", "mar"],
        pt: &["marcos", "mc", "mr", "mar"],
        fr: &["marc", "mc", "mr"],
        ko: &["마가복음", "마가", "막"],
        yo: &["máàkù", "maaku", "maku", "mak"],
    },
    BookSpec {
        book_num: 42,
        code: "LUK",
        osis: "Luke",
        name: "Luke",
        is_single_chapter: false,
        en: &["luke", "luk", "lk"],
        es: &["lucas", "lc", "luc"],
        pt: &["lucas", "lc", "luc"],
        fr: &["luc", "lc"],
        ko: &["누가복음", "누가", "눅"],
        yo: &["lúùkù", "luku", "luk"],
    },
    BookSpec {
        book_num: 43,
        code: "JHN",
        osis: "John",
        name: "John",
        is_single_chapter: false,
        en: &["john", "jhn", "jn", "joh"],
        es: &["juan", "jn"],
        pt: &["joão", "joao", "jo"],
        fr: &["jean", "jn"],
        ko: &["요한복음", "요한", "요"],
        yo: &["jòhánù", "jọọnu", "johanu", "jọh", "joh"],
    },
    BookSpec {
        book_num: 44,
        code: "ACT",
        osis: "Acts",
        name: "Acts",
        is_single_chapter: false,
        en: &["acts", "acts of the apostles", "act", "ac"],
        es: &["hechos", "hechos de los apóstoles", "hch", "hech", "hec"],
        pt: &["atos", "atos dos apóstolos", "at"],
        fr: &["actes", "actes des apôtres", "ac", "act"],
        ko: &["사도행전", "행"],
        yo: &[
            "ìṣe àwọn àpọ́sítélì",
            "iṣe awọn apọsiteli",
            "ise awon aposteli",
            "iṣe",
            "ise",
        ],
    },
    BookSpec {
        book_num: 45,
        code: "ROM",
        osis: "Rom",
        name: "Romans",
        is_single_chapter: false,
        en: &["romans", "rom", "ro", "rm"],
        es: &["romanos", "ro", "rom", "rm"],
        pt: &["romanos", "rm", "rom", "ro"],
        fr: &["romains", "rm", "ro", "rom"],
        ko: &["로마서", "롬"],
        yo: &["róòmù", "romu", "rom"],
    },
    BookSpec {
        book_num: 46,
        code: "1CO",
        osis: "1Cor",
        name: "1 Corinthians",
        is_single_chapter: false,
        en: &[
            "1 corinthians",
            "1 cor",
            "1 co",
            "1co",
            "i corinthians",
            "i cor",
            "first corinthians",
        ],
        es: &["1 corintios", "1 co", "1 cor", "i corintios"],
        pt: &["1 coríntios", "1 corintios", "1 co", "1 cor", "i coríntios"],
        fr: &["1 corinthiens", "1 co", "1 cor", "i corinthiens"],
        ko: &["고린도전서", "고전"],
        yo: &["1 kọ́ríńtì", "1 kọrinti", "1 korinti", "1 kọr", "1 kor"],
    },
    BookSpec {
        book_num: 47,
        code: "2CO",
        osis: "2Cor",
        name: "2 Corinthians",
        is_single_chapter: false,
        en: &[
            "2 corinthians",
            "2 cor",
            "2 co",
            "2co",
            "ii corinthians",
            "ii cor",
            "second corinthians",
        ],
        es: &["2 corintios", "2 co", "2 cor", "ii corintios"],
        pt: &[
            "2 coríntios",
            "2 corintios",
            "2 co",
            "2 cor",
            "ii coríntios",
        ],
        fr: &["2 corinthiens", "2 co", "2 cor", "ii corinthiens"],
        ko: &["고린도후서", "고후"],
        yo: &["2 kọ́ríńtì", "2 kọrinti", "2 korinti", "2 kọr", "2 kor"],
    },
    BookSpec {
        book_num: 48,
        code: "GAL",
        osis: "Gal",
        name: "Galatians",
        is_single_chapter: false,
        en: &["galatians", "gal", "ga"],
        es: &["gálatas", "galatas", "gál", "gal"],
        pt: &["gálatas", "galatas", "gl", "gál", "gal"],
        fr: &["galates", "ga", "gal"],
        ko: &["갈라디아서", "갈"],
        yo: &["gálátíà", "galatia", "gal"],
    },
    BookSpec {
        book_num: 49,
        code: "EPH",
        osis: "Eph",
        name: "Ephesians",
        is_single_chapter: false,
        en: &["ephesians", "eph", "ephes"],
        es: &["efesios", "ef", "efe"],
        pt: &["efésios", "efesios", "ef", "efe"],
        fr: &["éphésiens", "ephesiens", "ép", "ep", "éph", "eph"],
        ko: &["에베소서", "엡"],
        yo: &["éfésù", "efesu", "efe"],
    },
    BookSpec {
        book_num: 50,
        code: "PHP",
        osis: "Phil",
        name: "Philippians",
        is_single_chapter: false,
        en: &["philippians", "phil", "php", "pp"],
        es: &["filipenses", "flp", "fil"],
        pt: &["filipenses", "fp", "flp", "fil"],
        fr: &["philippiens", "ph", "php", "phil"],
        ko: &["빌립보서", "빌"],
        yo: &["fílípì", "filipi", "fil"],
    },
    BookSpec {
        book_num: 51,
        code: "COL",
        osis: "Col",
        name: "Colossians",
        is_single_chapter: false,
        en: &["colossians", "col", "co"],
        es: &["colosenses", "col"],
        pt: &["colossenses", "cl", "col"],
        fr: &["colossiens", "col"],
        ko: &["골로새서", "골"],
        yo: &["kólósè", "kolose", "kol"],
    },
    BookSpec {
        book_num: 52,
        code: "1TH",
        osis: "1Thess",
        name: "1 Thessalonians",
        is_single_chapter: false,
        en: &[
            "1 thessalonians",
            "1 thess",
            "1 thes",
            "1 th",
            "1th",
            "i thessalonians",
        ],
        es: &["1 tesalonicenses", "1 ts", "1 tes", "i tesalonicenses"],
        pt: &["1 tessalonicenses", "1 ts", "1 tes", "i tessalonicenses"],
        fr: &["1 thessaloniciens", "1 th", "1 thes", "i thessaloniciens"],
        ko: &["데살로니가전서", "살전"],
        yo: &[
            "1 tẹsalóníkà",
            "1 tẹsalonika",
            "1 tesalonika",
            "1 tẹs",
            "1 tes",
        ],
    },
    BookSpec {
        book_num: 53,
        code: "2TH",
        osis: "2Thess",
        name: "2 Thessalonians",
        is_single_chapter: false,
        en: &[
            "2 thessalonians",
            "2 thess",
            "2 thes",
            "2 th",
            "2th",
            "ii thessalonians",
        ],
        es: &["2 tesalonicenses", "2 ts", "2 tes", "ii tesalonicenses"],
        pt: &["2 tessalonicenses", "2 ts", "2 tes", "ii tessalonicenses"],
        fr: &["2 thessaloniciens", "2 th", "2 thes", "ii thessaloniciens"],
        ko: &["데살로니가후서", "살후"],
        yo: &[
            "2 tẹsalóníkà",
            "2 tẹsalonika",
            "2 tesalonika",
            "2 tẹs",
            "2 tes",
        ],
    },
    BookSpec {
        book_num: 54,
        code: "1TI",
        osis: "1Tim",
        name: "1 Timothy",
        is_single_chapter: false,
        en: &["1 timothy", "1 tim", "1 ti", "1ti", "i timothy", "i tim"],
        es: &["1 timoteo", "1 ti", "1 tim", "i timoteo"],
        pt: &["1 timóteo", "1 timoteo", "1 tm", "1 tim", "i timóteo"],
        fr: &["1 timothée", "1 timothee", "1 tm", "1 tim", "i timothée"],
        ko: &["디모데전서", "딤전"],
        yo: &["1 tímótì", "1 timotiu", "1 tim"],
    },
    BookSpec {
        book_num: 55,
        code: "2TI",
        osis: "2Tim",
        name: "2 Timothy",
        is_single_chapter: false,
        en: &["2 timothy", "2 tim", "2 ti", "2ti", "ii timothy", "ii tim"],
        es: &["2 timoteo", "2 ti", "2 tim", "ii timoteo"],
        pt: &["2 timóteo", "2 timoteo", "2 tm", "2 tim", "ii timóteo"],
        fr: &["2 timothée", "2 timothee", "2 tm", "2 tim", "ii timothée"],
        ko: &["디모데후서", "딤후"],
        yo: &["2 tímótì", "2 timotiu", "2 tim"],
    },
    BookSpec {
        book_num: 56,
        code: "TIT",
        osis: "Titus",
        name: "Titus",
        is_single_chapter: false,
        en: &["titus", "tit", "ti"],
        es: &["tito", "tit", "tt"],
        pt: &["tito", "tt", "tit"],
        fr: &["tite", "tt", "tit"],
        ko: &["디도서", "딛"],
        yo: &["títù", "titu", "tit"],
    },
    BookSpec {
        book_num: 57,
        code: "PHM",
        osis: "Phlm",
        name: "Philemon",
        is_single_chapter: true,
        en: &["philemon", "philem", "phlm", "phm", "pm"],
        es: &["filemón", "filemon", "flm", "film"],
        pt: &["filemom", "filemon", "fm", "flm"],
        fr: &["philémon", "philemon", "phm", "phlm"],
        ko: &["빌레몬서", "몬"],
        yo: &["fílémọ́nì", "filemọni", "filemoni", "flm"],
    },
    BookSpec {
        book_num: 58,
        code: "HEB",
        osis: "Heb",
        name: "Hebrews",
        is_single_chapter: false,
        en: &["hebrews", "heb", "he"],
        es: &["hebreos", "heb", "he"],
        pt: &["hebreus", "hb", "heb"],
        fr: &["hébreux", "hebreux", "hé", "he", "héb", "heb"],
        ko: &["히브리서", "히"],
        yo: &["hébérù", "heberu", "heb"],
    },
    BookSpec {
        book_num: 59,
        code: "JAS",
        osis: "Jas",
        name: "James",
        is_single_chapter: false,
        en: &["james", "jas", "jm", "ja"],
        es: &["santiago", "stg", "st", "sant"],
        pt: &["tiago", "tg"],
        fr: &["jacques", "jc", "jq", "jac", "jacq"],
        ko: &["야고보서", "약"],
        yo: &["jákọ́bù", "jakọbu", "jakobu", "jak"],
    },
    BookSpec {
        book_num: 60,
        code: "1PE",
        osis: "1Pet",
        name: "1 Peter",
        is_single_chapter: false,
        en: &[
            "1 peter", "1 pet", "1 pe", "1 pt", "1p", "1pe", "i peter", "i pet",
        ],
        es: &["1 pedro", "1 p", "1 pe", "1 ped", "i pedro"],
        pt: &["1 pedro", "1 pe", "1 pd", "i pedro"],
        fr: &["1 pierre", "1 p", "1 pi", "1 pie", "i pierre"],
        ko: &["베드로전서", "벧전"],
        yo: &["1 pétérù", "1 peteru", "1 pet"],
    },
    BookSpec {
        book_num: 61,
        code: "2PE",
        osis: "2Pet",
        name: "2 Peter",
        is_single_chapter: false,
        en: &[
            "2 peter", "2 pet", "2 pe", "2 pt", "2p", "2pe", "ii peter", "ii pet",
        ],
        es: &["2 pedro", "2 p", "2 pe", "2 ped", "ii pedro"],
        pt: &["2 pedro", "2 pe", "2 pd", "ii pedro"],
        fr: &["2 pierre", "2 p", "2 pi", "2 pie", "ii pierre"],
        ko: &["베드로후서", "벧후"],
        yo: &["2 pétérù", "2 peteru", "2 pet"],
    },
    BookSpec {
        book_num: 62,
        code: "1JN",
        osis: "1John",
        name: "1 John",
        is_single_chapter: false,
        en: &["1 john", "1 jhn", "1 jn", "1 jo", "1jn", "i john", "i jn"],
        es: &["1 juan", "1 jn", "i juan"],
        pt: &["1 joão", "1 joao", "1 jo", "i joão"],
        fr: &["1 jean", "1 jn", "i jean"],
        ko: &["요한일서", "요한1서", "요일"],
        yo: &["1 jòhánù", "1 jọọnu", "1 johanu", "1 jọh", "1 joh"],
    },
    BookSpec {
        book_num: 63,
        code: "2JN",
        osis: "2John",
        name: "2 John",
        is_single_chapter: true,
        en: &["2 john", "2 jhn", "2 jn", "2 jo", "2jn", "ii john", "ii jn"],
        es: &["2 juan", "2 jn", "ii juan"],
        pt: &["2 joão", "2 joao", "2 jo", "ii joão"],
        fr: &["2 jean", "2 jn", "ii jean"],
        ko: &["요한이서", "요한2서", "요이"],
        yo: &["2 jòhánù", "2 jọọnu", "2 johanu", "2 jọh", "2 joh"],
    },
    BookSpec {
        book_num: 64,
        code: "3JN",
        osis: "3John",
        name: "3 John",
        is_single_chapter: true,
        en: &[
            "3 john", "3 jhn", "3 jn", "3 jo", "3jn", "iii john", "iii jn",
        ],
        es: &["3 juan", "3 jn", "iii juan"],
        pt: &["3 joão", "3 joao", "3 jo", "iii joão"],
        fr: &["3 jean", "3 jn", "iii jean"],
        ko: &["요한삼서", "요한3서", "요삼"],
        yo: &["3 jòhánù", "3 jọọnu", "3 johanu", "3 jọh", "3 joh"],
    },
    BookSpec {
        book_num: 65,
        code: "JUD",
        osis: "Jude",
        name: "Jude",
        is_single_chapter: true,
        en: &["jude", "jud", "jd"],
        es: &["judas", "jud", "jds"],
        pt: &["judas", "jd", "jud"],
        fr: &["jude", "jd", "jud"],
        ko: &["유다서", "유"],
        yo: &["juda", "júdà", "jud"],
    },
    BookSpec {
        book_num: 66,
        code: "REV",
        osis: "Rev",
        name: "Revelation",
        is_single_chapter: false,
        en: &[
            "revelation",
            "revelations",
            "rev",
            "re",
            "rv",
            "apoc",
            "apocalypse",
        ],
        es: &["apocalipsis", "ap", "apoc"],
        pt: &["apocalipse", "ap", "apoc"],
        fr: &["apocalypse", "ap", "apoc"],
        ko: &["요한계시록", "계시록", "계"],
        yo: &["ìfihàn", "ifihan", "ifi"],
    },
];

pub fn book_by_num(book_num: u32) -> Option<&'static BookSpec> {
    book_num
        .checked_sub(1)
        .and_then(|idx| BOOKS.get(idx as usize))
}

pub fn book_by_code(code: &str) -> Option<&'static BookSpec> {
    let trimmed = code.trim();
    BOOKS.iter().find(|b| b.code.eq_ignore_ascii_case(trimmed))
}

pub fn book_by_osis(osis: &str) -> Option<&'static BookSpec> {
    let trimmed = osis.trim();
    BOOKS
        .iter()
        .find(|b| b.osis.eq_ignore_ascii_case(trimmed) || b.code.eq_ignore_ascii_case(trimmed))
}

#[derive(Default)]
struct AliasIndex {
    any_exact: HashMap<String, u32>,
    by_lang_exact: HashMap<&'static str, HashMap<String, u32>>,
    any: HashMap<String, u32>,
    by_lang: HashMap<&'static str, HashMap<String, u32>>,
}

static ALIAS_INDEX: OnceLock<AliasIndex> = OnceLock::new();

fn normalize_space_and_ordinals(input: &str) -> String {
    let mut cleaned = String::with_capacity(input.len());
    let mut prev_space = true;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                cleaned.push(lower);
            }
            prev_space = false;
        } else if (ch.is_whitespace() || ch == '.' || ch == '-' || ch == '\'') && !prev_space {
            cleaned.push(' ');
            prev_space = true;
        }
    }
    let trimmed = cleaned.trim();
    // Normalize ordinal prefixes like "1st corinthians", "1ª joão", "1re rois" -> "1 ..."
    for (prefix, replacement) in [
        ("1st ", "1 "),
        ("first ", "1 "),
        ("1a ", "1 "),
        ("1ª ", "1 "),
        ("1º ", "1 "),
        ("1ra ", "1 "),
        ("1re ", "1 "),
        ("1ere ", "1 "),
        ("1ère ", "1 "),
        ("2nd ", "2 "),
        ("second ", "2 "),
        ("2a ", "2 "),
        ("2ª ", "2 "),
        ("2º ", "2 "),
        ("2da ", "2 "),
        ("2e ", "2 "),
        ("2eme ", "2 "),
        ("2ème ", "2 "),
        ("3rd ", "3 "),
        ("third ", "3 "),
        ("3a ", "3 "),
        ("3ª ", "3 "),
        ("3º ", "3 "),
        ("3ra ", "3 "),
        ("3e ", "3 "),
        ("3eme ", "3 "),
        ("3ème ", "3 "),
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return format!("{replacement}{rest}");
        }
    }
    trimmed.to_owned()
}

pub(crate) fn normalize_exact_alias_key(raw: &str) -> String {
    normalize_space_and_ordinals(raw)
}

pub(crate) fn normalize_alias_key(raw: &str) -> String {
    let folded = fold_text(raw);
    normalize_space_and_ordinals(&folded)
}

fn expand_roman_variants(key: &str) -> Vec<String> {
    let mut variants = vec![key.to_owned()];
    for (num_prefix, roman_prefixes) in [
        ("1 ", &["i ", "1"][..]),
        ("2 ", &["ii ", "2"][..]),
        ("3 ", &["iii ", "3"][..]),
    ] {
        if let Some(rest) = key.strip_prefix(num_prefix) {
            for r in roman_prefixes {
                variants.push(format!("{r}{rest}"));
            }
        }
    }
    variants
}

type LangAliasGetter = fn(&BookSpec) -> &'static [&'static str];

fn alias_index() -> &'static AliasIndex {
    ALIAS_INDEX.get_or_init(|| {
        let mut idx = AliasIndex::default();
        let langs: [(&'static str, LangAliasGetter); 6] = [
            ("en", |b| b.en),
            ("es", |b| b.es),
            ("pt", |b| b.pt),
            ("fr", |b| b.fr),
            ("ko", |b| b.ko),
            ("yo", |b| b.yo),
        ];

        // Seed canonical codes and OSIS IDs into fallback map first.
        for book in &BOOKS {
            idx.any
                .entry(normalize_alias_key(book.code))
                .or_insert(book.book_num);
            idx.any
                .entry(normalize_alias_key(book.osis))
                .or_insert(book.book_num);
            idx.any
                .entry(normalize_alias_key(book.name))
                .or_insert(book.book_num);
        }

        // Insert per-language aliases (exact diacritics first, then folded fallback).
        for (lang, getter) in langs {
            let exact_map = idx.by_lang_exact.entry(lang).or_default();
            let map = idx.by_lang.entry(lang).or_default();
            for book in &BOOKS {
                for alias in getter(book) {
                    let exact_key = normalize_exact_alias_key(alias);
                    for variant in expand_roman_variants(&exact_key) {
                        exact_map.entry(variant.clone()).or_insert(book.book_num);
                        idx.any_exact.entry(variant).or_insert(book.book_num);
                    }
                    let key = normalize_alias_key(alias);
                    for variant in expand_roman_variants(&key) {
                        // In Portuguese, keep unaccented "jo" reserved for João (John) while "jó" hits Job via exact map.
                        if lang == "pt" && book.code == "JOB" && variant == "jo" {
                            continue;
                        }
                        map.entry(variant.clone()).or_insert(book.book_num);
                        idx.any.entry(variant).or_insert(book.book_num);
                    }
                }
            }
        }
        idx
    })
}

fn lookup_map_with_compact(map: &HashMap<String, u32>, key: &str) -> Option<u32> {
    if let Some(&num) = map.get(key) {
        return Some(num);
    }
    let compact: String = key.chars().filter(|c| !c.is_whitespace()).collect();
    map.get(&compact).copied()
}

/// Resolve a book name or abbreviation across supported languages (`en`, `es`, `pt`, `fr`, `ko`, `yo`).
pub fn resolve_book_alias(raw: &str, language_hint: Option<&str>) -> Option<&'static BookSpec> {
    let exact_key = normalize_exact_alias_key(raw);
    let key = normalize_alias_key(raw);
    if key.is_empty() && exact_key.is_empty() {
        return None;
    }
    let idx = alias_index();
    let lang = language_hint
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.split(['-', '_']).next().unwrap_or(l).to_ascii_lowercase());

    if let Some(code) = lang.as_deref() {
        if let Some(exact_map) = idx.by_lang_exact.get(code)
            && let Some(num) = lookup_map_with_compact(exact_map, &exact_key)
        {
            return book_by_num(num);
        }
        if let Some(lang_map) = idx.by_lang.get(code)
            && let Some(num) = lookup_map_with_compact(lang_map, &key)
        {
            return book_by_num(num);
        }
    }

    if let Some(num) = lookup_map_with_compact(&idx.any_exact, &exact_key) {
        return book_by_num(num);
    }
    lookup_map_with_compact(&idx.any, &key).and_then(book_by_num)
}
