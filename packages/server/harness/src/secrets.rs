use regex::Regex;
use std::sync::{LazyLock, RwLock};

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i:Bearer)[\t ]+(?:[^\s]+|$)|sk-[A-Za-z0-9_-]{16,}|[[:xdigit:]]{64,}").unwrap()
});

/// Shared by the process reader (stderr) and the service (public state).
#[derive(Default)]
pub struct Secrets(RwLock<Vec<String>>);

impl Secrets {
    pub fn remember(&self, value: &str) {
        let mut values = self.0.write().unwrap();
        if !value.is_empty() && !values.iter().any(|s| s == value) {
            values.push(value.to_owned());
        }
    }

    pub fn text(&self, text: &str) -> String {
        self.prefix(text, true).0
    }

    fn prefix(&self, text: &str, end: bool) -> (String, usize) {
        let values = self.0.read().unwrap();
        // 64 bytes also cover the prefix needed to recognize a generic hex token.
        let tail = values.iter().map(String::len).max().unwrap_or(0).max(64);
        let mut cut = if end {
            text.len()
        } else {
            text.floor_char_boundary(text.len().saturating_sub(tail))
        };
        if cut == 0 {
            return (String::new(), 0);
        }
        let mut spans: Vec<_> = TOKENS
            .find_iter(text)
            .map(|m| (m.start(), m.end()))
            .collect();
        for secret in values.iter() {
            spans.extend(text.match_indices(secret).map(|(i, s)| (i, i + s.len())));
        }
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, stop) in spans {
            if let Some(last) = merged.last_mut()
                && start < last.1
            {
                last.1 = last.1.max(stop);
            } else {
                merged.push((start, stop));
            }
        }
        let mut out = String::new();
        let mut pos = 0;
        for (start, stop) in merged {
            if start >= cut {
                break;
            }
            if stop > cut {
                cut = start;
                break;
            }
            out.push_str(&text[pos..start]);
            out.push_str("[redacted]");
            pos = stop;
        }
        out.push_str(&text[pos..cut]);
        (out, cut)
    }
}

#[derive(Default)]
pub struct StreamText {
    tail: String,
    next_scan: usize,
}
impl StreamText {
    pub fn push(&mut self, secrets: &Secrets, delta: &str) -> String {
        self.tail.push_str(delta);
        if self.tail.len() < self.next_scan {
            return String::new();
        }
        let (out, consumed) = secrets.prefix(&self.tail, false);
        self.tail.drain(..consumed);
        // Long unterminated credentials must not make each new byte rescan the entire prefix.
        self.next_scan = if consumed == 0 {
            self.tail.len().saturating_mul(2)
        } else {
            0
        };
        out
    }
    pub fn finish(self, secrets: &Secrets) -> String {
        secrets.text(&self.tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_boundaries_preserve_exact_generic_and_utf8_matches() {
        let secrets = Secrets::default();
        let literal = "specific credential ".repeat(6);
        secrets.remember(&literal);
        for value in [
            literal,
            format!("sk-{}", "Z".repeat(250)),
            format!("Bearer {}{}", " ".repeat(90), "J".repeat(150)),
            "ab".repeat(512),
        ] {
            let head = "常规 output ".repeat(50);
            let tail = " /end".repeat(50);
            let raw = format!("{head}{value}{tail}");
            for cut in (head.len()..=head.len() + value.len()).filter(|i| raw.is_char_boundary(*i))
            {
                let mut stream = StreamText::default();
                let mut out = stream.push(&secrets, &raw[..cut]);
                out.push_str(&stream.push(&secrets, &raw[cut..]));
                out.push_str(&stream.finish(&secrets));
                assert_eq!(out, format!("{head}[redacted]{tail}"));
            }
        }
    }
}
