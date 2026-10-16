//! Selection over the editor's invisible OCR text layer (replaces the
//! browser's text selection in `TextLayer.tsx`). Selection is word-granular;
//! `selectedText` already copied whole words, so the copied text is the same.

use crate::ocr::TextLayout;

/// A selectable box in image pixels: a word, or a whole line without words.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub line: usize,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Glyph {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    fn distance(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(0.0).max(x - (self.x + self.width));
        let dy = (self.y - y).max(0.0).max(y - (self.y + self.height));
        dx.hypot(dy)
    }
}

/// Glyphs in reading order (line, then word), like the DOM order of the spans.
pub fn glyphs(layout: &TextLayout) -> Vec<Glyph> {
    let (width, height) = (layout.width, layout.height);
    layout
        .lines
        .iter()
        .enumerate()
        .flat_map(|(line, text_line)| {
            let boxes: Vec<(&str, f64, f64, f64, f64)> = if text_line.words.is_empty() {
                vec![(
                    &text_line.text,
                    text_line.x,
                    text_line.y,
                    text_line.w,
                    text_line.h,
                )]
            } else {
                text_line
                    .words
                    .iter()
                    .map(|word| (word.text.as_str(), word.x, word.y, word.w, word.h))
                    .collect()
            };
            boxes.into_iter().map(move |(text, x, y, w, h)| Glyph {
                line,
                text: text.to_string(),
                x: x * width,
                y: y * height,
                width: w * width,
                height: h * height,
            })
        })
        .collect()
}

/// The glyph under a point, if any.
pub fn hit_test(glyphs: &[Glyph], x: f64, y: f64) -> Option<usize> {
    glyphs.iter().position(|glyph| glyph.contains(x, y))
}

/// The glyph a drag at this point extends to: the one under it, else the
/// closest one.
pub fn nearest(glyphs: &[Glyph], x: f64, y: f64) -> Option<usize> {
    hit_test(glyphs, x, y).or_else(|| {
        glyphs
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.distance(x, y).total_cmp(&b.distance(x, y)))
            .map(|(index, _)| index)
    })
}

/// Anchor → focus over the reading-order glyph sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    /// Inclusive glyph range in reading order, whichever way it was dragged.
    pub fn range(&self) -> std::ops::RangeInclusive<usize> {
        self.anchor.min(self.focus)..=self.anchor.max(self.focus)
    }

    pub fn contains(&self, index: usize) -> bool {
        self.range().contains(&index)
    }

    /// Double-click: the word under the pointer.
    pub fn word(index: usize) -> Self {
        Self::new(index, index)
    }

    /// Triple-click: every glyph on the line under the pointer.
    pub fn line(glyphs: &[Glyph], index: usize) -> Self {
        let line = glyphs[index].line;
        let first = glyphs
            .iter()
            .position(|glyph| glyph.line == line)
            .unwrap_or(index);
        let last = glyphs
            .iter()
            .rposition(|glyph| glyph.line == line)
            .unwrap_or(index);
        Self::new(first, last)
    }
}

/// `selectedText()`: words joined with spaces within a line, lines with `\n`.
pub fn selected_text(glyphs: &[Glyph], selection: Selection) -> String {
    let mut lines: Vec<(usize, Vec<&str>)> = Vec::new();
    for glyph in &glyphs[selection.range()] {
        match lines.last_mut() {
            Some((line, words)) if *line == glyph.line => words.push(&glyph.text),
            _ => lines.push((glyph.line, vec![&glyph.text])),
        }
    }
    lines
        .into_iter()
        .map(|(_, words)| words.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{TextBox, TextLine};

    fn word(text: &str, x: f64, y: f64) -> TextBox {
        TextBox {
            text: text.into(),
            x,
            y,
            w: 0.1,
            h: 0.05,
        }
    }

    fn layout() -> TextLayout {
        TextLayout {
            width: 1000.0,
            height: 500.0,
            lines: vec![
                TextLine {
                    text: "Hello brave world".into(),
                    x: 0.1,
                    y: 0.1,
                    w: 0.5,
                    h: 0.05,
                    words: vec![
                        word("Hello", 0.1, 0.1),
                        word("brave", 0.25, 0.1),
                        word("world", 0.4, 0.1),
                    ],
                },
                TextLine {
                    text: "→".into(),
                    x: 0.1,
                    y: 0.3,
                    w: 0.05,
                    h: 0.05,
                    words: Vec::new(),
                },
                TextLine {
                    text: "Second line".into(),
                    x: 0.1,
                    y: 0.5,
                    w: 0.3,
                    h: 0.05,
                    words: vec![word("Second", 0.1, 0.5), word("line", 0.25, 0.5)],
                },
            ],
        }
    }

    #[test]
    fn glyphs_use_words_or_the_whole_line_in_image_pixels() {
        let glyphs = glyphs(&layout());
        let texts: Vec<_> = glyphs.iter().map(|glyph| glyph.text.as_str()).collect();
        assert_eq!(texts, ["Hello", "brave", "world", "→", "Second", "line"]);
        assert_eq!(glyphs[1].x, 250.0);
        assert_eq!(glyphs[1].y, 50.0);
        assert_eq!(
            (glyphs[3].line, glyphs[3].width, glyphs[3].height),
            (1, 50.0, 25.0)
        );
    }

    #[test]
    fn hit_testing_finds_the_word_under_the_pointer() {
        let glyphs = glyphs(&layout());
        assert_eq!(hit_test(&glyphs, 260.0, 60.0), Some(1));
        assert_eq!(hit_test(&glyphs, 900.0, 400.0), None);
        assert_eq!(nearest(&glyphs, 900.0, 60.0), Some(2));
        assert_eq!(nearest(&glyphs, 0.0, 260.0), Some(4));
    }

    #[test]
    fn selections_are_ordered_ranges_in_reading_order() {
        let glyphs = glyphs(&layout());
        let forward = Selection::new(1, 4);
        let backward = Selection::new(4, 1);
        assert_eq!(forward.range(), backward.range());
        assert_eq!(selected_text(&glyphs, backward), "brave world\n→\nSecond");
        assert_eq!(selected_text(&glyphs, Selection::word(5)), "line");
    }

    #[test]
    fn triple_click_selects_the_line() {
        let glyphs = glyphs(&layout());
        let line = Selection::line(&glyphs, 1);
        assert_eq!(line.range(), 0..=2);
        assert_eq!(selected_text(&glyphs, line), "Hello brave world");
        assert_eq!(selected_text(&glyphs, Selection::line(&glyphs, 3)), "→");
    }
}
