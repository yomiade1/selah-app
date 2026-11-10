//! Rule-based scripture verse-to-slide splitter (M2.5 / ARCHITECTURE §4.3).
use anyhow::ensure;
use selah_proto::v1::{
    GeneratedScriptureSlide, GeneratedScriptureSlides, ReferencePlacement, ScriptureVerse,
    SplitScriptureSlides,
};

use super::reference::format_span;
use super::store::MAX_PASSAGE_VERSES;

pub const DEFAULT_SCRIPTURE_MAX_LINES: u32 = 4;
pub const MAX_SCRIPTURE_SLIDE_LINES: u32 = 100;
pub const DEFAULT_SCRIPTURE_LINE_CHARS: u32 = 48;
pub const MIN_SCRIPTURE_LINE_CHARS: u32 = 16;
pub const MAX_SCRIPTURE_LINE_CHARS: u32 = 200;
pub const MAX_VERSES_PER_SLIDE: u32 = 20;
const MAX_VERSE_BYTES: usize = 16 * 1024;

pub(crate) fn wrap_words(text: &str, max_chars: usize) -> Vec<String> {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;

    for word in normalized.split(' ') {
        let word_chars = word.chars().count();
        if current.is_empty() {
            if word_chars <= max_chars {
                current.push_str(word);
                current_chars = word_chars;
            } else {
                // Break overlong token cleanly along Unicode scalar boundaries.
                let mut chunk = String::new();
                let mut chunk_len = 0usize;
                for ch in word.chars() {
                    if chunk_len == max_chars {
                        lines.push(chunk);
                        chunk = String::new();
                        chunk_len = 0;
                    }
                    chunk.push(ch);
                    chunk_len += 1;
                }
                if !chunk.is_empty() {
                    current = chunk;
                    current_chars = chunk_len;
                }
            }
        } else if current_chars + 1 + word_chars <= max_chars {
            current.push(' ');
            current.push_str(word);
            current_chars += 1 + word_chars;
        } else {
            lines.push(current);
            if word_chars <= max_chars {
                current = word.to_owned();
                current_chars = word_chars;
            } else {
                current = String::new();
                current_chars = 0;
                let mut chunk = String::new();
                let mut chunk_len = 0usize;
                for ch in word.chars() {
                    if chunk_len == max_chars {
                        lines.push(chunk);
                        chunk = String::new();
                        chunk_len = 0;
                    }
                    chunk.push(ch);
                    chunk_len += 1;
                }
                if !chunk.is_empty() {
                    current = chunk;
                    current_chars = chunk_len;
                }
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn format_slide_verse_label(verses: &[&ScriptureVerse], translation: &str) -> String {
    if verses.is_empty() {
        return String::new();
    }
    let mut segments: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < verses.len() {
        let start = verses[i];
        let mut end_idx = i;
        while end_idx + 1 < verses.len()
            && verses[end_idx + 1].book_num == start.book_num
            && verses[end_idx + 1].chapter == verses[end_idx].chapter
            && verses[end_idx + 1].verse <= verses[end_idx].verse + 1
        {
            end_idx += 1;
        }
        let end = verses[end_idx];
        segments.push(format_span(
            &start.book_name,
            start.chapter,
            start.verse,
            end.chapter,
            end.verse,
        ));
        i = end_idx + 1;
    }
    let joined = segments.join("; ");
    let t = translation.trim();
    if t.is_empty() {
        joined
    } else {
        format!("{joined} ({t})")
    }
}

fn attach_reference_slot(placement: ReferencePlacement, slide_ref: &str) -> (String, String) {
    match placement {
        ReferencePlacement::None => (String::new(), String::new()),
        ReferencePlacement::SlideHeader => (slide_ref.to_owned(), String::new()),
        ReferencePlacement::SlideFooter | ReferencePlacement::Unspecified => {
            (String::new(), slide_ref.to_owned())
        }
    }
}

pub fn split_scripture_slides(
    req: &SplitScriptureSlides,
) -> anyhow::Result<GeneratedScriptureSlides> {
    ensure!(
        !req.translation.contains('\0')
            && !req.translation_name.contains('\0')
            && !req.reference.contains('\0'),
        "scripture slide request contains a NUL byte"
    );
    ensure!(
        req.verses.len() <= MAX_PASSAGE_VERSES,
        "cannot split more than {MAX_PASSAGE_VERSES} verses at once"
    );
    for v in &req.verses {
        ensure!(!v.text.contains('\0'), "verse text contains a NUL byte");
        ensure!(
            v.text.len() <= MAX_VERSE_BYTES,
            "verse text exceeds {MAX_VERSE_BYTES} bytes"
        );
    }

    let max_lines = if req.max_lines == 0 {
        DEFAULT_SCRIPTURE_MAX_LINES
    } else {
        req.max_lines
    };
    ensure!(
        (1..=MAX_SCRIPTURE_SLIDE_LINES).contains(&max_lines),
        "max_lines must be between 1 and {MAX_SCRIPTURE_SLIDE_LINES}"
    );
    let max_chars = if req.max_chars_per_line == 0 {
        DEFAULT_SCRIPTURE_LINE_CHARS
    } else {
        req.max_chars_per_line
    };
    ensure!(
        (MIN_SCRIPTURE_LINE_CHARS..=MAX_SCRIPTURE_LINE_CHARS).contains(&max_chars),
        "max_chars_per_line must be between {MIN_SCRIPTURE_LINE_CHARS} and {MAX_SCRIPTURE_LINE_CHARS}"
    );
    ensure!(
        req.verses_per_slide <= MAX_VERSES_PER_SLIDE,
        "verses_per_slide cannot exceed {MAX_VERSES_PER_SLIDE}"
    );

    let placement = ReferencePlacement::try_from(req.reference_placement)
        .map_err(|_| anyhow::anyhow!("unknown reference placement"))?;

    let translation_code = req.translation.trim().to_owned();
    let translation_name = req.translation_name.trim().to_owned();
    let verse_refs: Vec<&ScriptureVerse> = req.verses.iter().collect();

    let base_ref = if !req.reference.trim().is_empty() {
        req.reference.trim().to_owned()
    } else {
        format_slide_verse_label(&verse_refs, "")
    };
    let passage_label = if base_ref.is_empty() {
        String::new()
    } else if translation_code.is_empty() || base_ref.ends_with(&format!("({translation_code})")) {
        base_ref
    } else {
        format!("{base_ref} ({translation_code})")
    };

    let verse_cap = if req.one_verse_per_slide {
        Some(1usize)
    } else if req.verses_per_slide > 0 {
        Some(req.verses_per_slide as usize)
    } else {
        None
    };

    let max_lines_usize = max_lines as usize;
    let max_chars_usize = max_chars as usize;

    struct DraftContentSlide<'a> {
        lines: Vec<String>,
        verses: Vec<&'a ScriptureVerse>,
    }

    let mut content_drafts: Vec<DraftContentSlide<'_>> = Vec::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut current_verses: Vec<&ScriptureVerse> = Vec::new();

    for verse in &req.verses {
        let display = if req.show_verse_numbers {
            format!("{} {}", verse.verse, verse.text.trim())
        } else {
            verse.text.trim().to_owned()
        };
        let wrapped = wrap_words(&display, max_chars_usize);
        if wrapped.is_empty() {
            continue;
        }

        let fits_current = !current_lines.is_empty()
            && current_lines.len() + wrapped.len() <= max_lines_usize
            && verse_cap.is_none_or(|cap| current_verses.len() < cap);

        if fits_current {
            current_lines.extend(wrapped);
            current_verses.push(verse);
            continue;
        }

        if !current_lines.is_empty() {
            content_drafts.push(DraftContentSlide {
                lines: std::mem::take(&mut current_lines),
                verses: std::mem::take(&mut current_verses),
            });
        }

        if wrapped.len() <= max_lines_usize {
            current_lines = wrapped;
            current_verses = vec![verse];
            if verse_cap == Some(1) {
                content_drafts.push(DraftContentSlide {
                    lines: std::mem::take(&mut current_lines),
                    verses: std::mem::take(&mut current_verses),
                });
            }
        } else {
            for chunk in wrapped.chunks(max_lines_usize) {
                content_drafts.push(DraftContentSlide {
                    lines: chunk.to_vec(),
                    verses: vec![verse],
                });
            }
        }
    }

    if !current_lines.is_empty() {
        content_drafts.push(DraftContentSlide {
            lines: current_lines,
            verses: current_verses,
        });
    }

    let mut slides = Vec::new();
    if req.title_slide {
        let mut lines = Vec::new();
        if !passage_label.is_empty() {
            lines.push(passage_label.clone());
        }
        if !translation_name.is_empty() {
            lines.push(translation_name.clone());
        }
        slides.push(GeneratedScriptureSlide {
            reference: passage_label.clone(),
            part_index: 1,
            part_count: 1,
            lines,
            header_reference: String::new(),
            footer_reference: String::new(),
            is_title: true,
            is_blank: false,
        });
    }

    let content_count = content_drafts.len() as u32;
    for (idx, draft) in content_drafts.into_iter().enumerate() {
        let slide_ref = format_slide_verse_label(&draft.verses, &translation_code);
        let (header_reference, footer_reference) = attach_reference_slot(placement, &slide_ref);
        slides.push(GeneratedScriptureSlide {
            reference: slide_ref,
            part_index: (idx + 1) as u32,
            part_count: content_count,
            lines: draft.lines,
            header_reference,
            footer_reference,
            is_title: false,
            is_blank: false,
        });
    }

    if req.blank_slide_at_end {
        slides.push(GeneratedScriptureSlide {
            reference: String::new(),
            part_index: 1,
            part_count: 1,
            lines: Vec::new(),
            header_reference: String::new(),
            footer_reference: String::new(),
            is_title: false,
            is_blank: true,
        });
    }

    Ok(GeneratedScriptureSlides {
        translation: translation_code,
        translation_name,
        reference_label: passage_label,
        slides,
    })
}
