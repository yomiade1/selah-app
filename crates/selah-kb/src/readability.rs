//! Reading level (KNOWLEDGE-BASE §7.1, R2–R4):
//! `grade = 0.39 × (words ÷ sentences) + 11.8 × (syllables ÷ words) − 15.59`.
use std::collections::HashMap;
use std::sync::OnceLock;

/// Exceptions to the syllable heuristic: `word count` per line.
fn exceptions() -> &'static HashMap<String, usize> {
    static TABLE: OnceLock<HashMap<String, usize>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/syllables.txt")
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let (word, count) = l.split_once(' ')?;
                Some((word.to_lowercase(), count.trim().parse().ok()?))
            })
            .collect()
    })
}

/// Syllables in one word: vowel groups, minus a silent final `e`, at least 1.
pub fn syllables(word: &str) -> usize {
    let word: String = word
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<String>()
        .to_lowercase();
    if word.is_empty() {
        return 0;
    }
    if let Some(count) = exceptions().get(&word) {
        return *count;
    }
    let vowel = |c: char| "aeiouy".contains(c);
    let chars: Vec<char> = word.chars().collect();
    let mut count = 0;
    let mut previous = false;
    for &c in &chars {
        let is = vowel(c);
        if is && !previous {
            count += 1;
        }
        previous = is;
    }
    let n = chars.len();
    // Silent e: "make", "time"; but "table", "people" keep their -le syllable.
    let consonant_le = n > 2 && chars[n - 2] == 'l' && !vowel(chars[n - 3]);
    if n > 2 && chars[n - 1] == 'e' && !vowel(chars[n - 2]) && !consonant_le {
        count -= 1;
    }
    // -ed after a consonant other than t or d is silent: "joined", "helped".
    if n > 3 && word.ends_with("ed") && !matches!(chars[n - 3], 't' | 'd') && !vowel(chars[n - 3]) {
        count -= 1;
    }
    // -es after most consonants is silent: "makes", "slides".
    if n > 3
        && word.ends_with("es")
        && !matches!(chars[n - 3], 's' | 'x' | 'z' | 'c' | 'g' | 'h')
        && !vowel(chars[n - 3])
    {
        count -= 1;
    }
    count.max(1)
}

pub fn words(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect()
}

/// Sentences in a piece of text (ends at . ! ? followed by a space or the end).
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        current.push(c);
        let at_end = i + 1 == chars.len() || chars[i + 1].is_whitespace();
        if matches!(c, '.' | '!' | '?') && at_end {
            if !words(&current).is_empty() {
                out.push(current.trim().to_owned());
            }
            current.clear();
        }
    }
    if !words(&current).is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub words: usize,
    pub sentences: usize,
    pub syllables: usize,
}

impl Stats {
    pub fn of(pieces: &[String]) -> Self {
        let mut stats = Stats::default();
        for piece in pieces {
            for sentence in sentences(piece) {
                let w = words(&sentence);
                stats.words += w.len();
                stats.sentences += 1;
                stats.syllables += w.iter().map(|w| syllables(w)).sum::<usize>();
            }
        }
        stats
    }

    pub fn grade(&self) -> f64 {
        if self.words == 0 || self.sentences == 0 {
            return 0.0;
        }
        0.39 * (self.words as f64 / self.sentences as f64)
            + 11.8 * (self.syllables as f64 / self.words as f64)
            - 15.59
    }

    pub fn average_sentence(&self) -> f64 {
        if self.sentences == 0 {
            0.0
        } else {
            self.words as f64 / self.sentences as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syllable_heuristic() {
        for (word, count) in [
            ("slide", 1),
            ("screen", 1),
            ("projector", 3),
            ("table", 2),
            ("joined", 1),
            ("people", 2),
            ("makes", 1),
            ("everything", 3),
            ("a", 1),
        ] {
            assert_eq!(syllables(word), count, "{word}");
        }
    }

    #[test]
    fn plain_text_scores_low_and_dense_text_high() {
        let plain =
            Stats::of(&["Pick the song. Then select a slide. It shows on the screens.".into()]);
        assert!(plain.grade() < 4.0, "{}", plain.grade());
        let dense = Stats::of(&["In order to configure edge blending functionality, navigate to the screens configuration panel and ensure that the projector overlap values are populated appropriately.".into()]);
        assert!(dense.grade() > 12.0, "{}", dense.grade());
    }

    #[test]
    fn sentences_split_on_end_marks() {
        assert_eq!(sentences("One. Two! Three? Version 1.0 works").len(), 4);
    }
}
