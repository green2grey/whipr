pub fn generate_title(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let first_line = trimmed
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(trimmed);

    let mut words = Vec::new();
    for word in first_line.split_whitespace() {
        words.push(word);
        if words.len() >= 8 {
            break;
        }
    }

    if words.is_empty() {
        return None;
    }

    let mut title = words.join(" ");
    title = title
        .trim_end_matches(|ch: char| ch.is_ascii_punctuation())
        .to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

pub fn generate_summary(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut sentences = Vec::new();
    let mut buffer = String::new();

    for ch in trimmed.chars() {
        buffer.push(ch);
        if ch == '.' || ch == '!' || ch == '?' {
            let sentence = buffer.trim().to_string();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            buffer.clear();
            if sentences.len() >= 2 {
                break;
            }
        }
    }

    if sentences.is_empty() {
        let words: Vec<&str> = trimmed.split_whitespace().take(24).collect();
        if words.is_empty() {
            return None;
        }
        return Some(words.join(" "));
    }

    let mut summary = sentences.join(" ");
    if summary.chars().count() > 200 {
        summary = summary.chars().take(200).collect::<String>();
        summary = summary.trim_end().to_string();
        summary.push_str("...");
    }

    Some(summary)
}
