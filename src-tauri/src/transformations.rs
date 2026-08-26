//! Deterministic output transformations, independent from speech recognition.

use serde::{Deserialize, Serialize};

use crate::output_policy::FormattingLevel;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormattingTarget {
    #[default]
    PlainText,
    Markdown,
    Code,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformationOutcome {
    pub text: String,
    pub changed: bool,
    pub warnings: Vec<String>,
}

pub fn apply_backtrack(text: &str, level: FormattingLevel) -> TransformationOutcome {
    if level == FormattingLevel::Literal {
        return unchanged(text);
    }
    let markers = [
        "... não, ",
        "… não, ",
        " não, ",
        "... quer dizer, ",
        "… quer dizer, ",
        " quer dizer, ",
    ];
    let lower = text.to_lowercase();
    let Some((position, marker)) = markers
        .iter()
        .filter_map(|marker| lower.rfind(marker).map(|position| (position, *marker)))
        .max_by_key(|(position, _)| *position)
    else {
        return unchanged(text);
    };
    let before = text[..position]
        .trim()
        .trim_end_matches(['.', ',', ';', ':']);
    let correction = text[position + marker.len()..].trim();
    if before.is_empty() || correction.is_empty() {
        return unchanged(text);
    }

    let relation_words = ["para ", "às ", "as ", "a ", "no ", "na ", "com ", "em "];
    let correction_lower = correction.to_lowercase();
    if let Some(relation) = relation_words
        .iter()
        .find(|relation| correction_lower.starts_with(**relation))
    {
        let before_lower = before.to_lowercase();
        if let Some(relation_position) = before_lower.rfind(relation) {
            let prefix = before[..relation_position].trim_end();
            let output = format!("{} {}", prefix, correction).trim().to_string();
            return changed(output, "backtrack_explicit_correction");
        }
    }

    if correction_lower.starts_with("deixa no ") || correction_lower.starts_with("deixa na ") {
        let target = correction
            .split_whitespace()
            .skip(2)
            .collect::<Vec<_>>()
            .join(" ");
        let first_word = before.split_whitespace().next().unwrap_or_default();
        if !target.is_empty()
            && matches!(first_word.to_lowercase().as_str(), "usa" | "use" | "usar")
        {
            return changed(
                format!("{first_word} {target}"),
                "backtrack_explicit_replacement",
            );
        }
    }
    unchanged(text)
}

pub fn apply_smart_formatting(
    text: &str,
    level: FormattingLevel,
    target: FormattingTarget,
) -> TransformationOutcome {
    let mut output = text.trim().to_string();
    if output.is_empty() {
        return unchanged(text);
    }
    output = replace_spoken_command(&output, &["novo parágrafo", "novo paragrafo"], "\n\n");
    output = replace_spoken_command(&output, &["nova linha", "quebra de linha"], "\n");

    if target == FormattingTarget::Markdown {
        output = explicit_list(&output);
    }
    if level != FormattingLevel::Literal && target != FormattingTarget::Code {
        output = capitalize_sentence_start(&output);
        if !output.ends_with(['.', '!', '?', ':', ';', '\n'])
            && !looks_like_code_or_identifier(&output)
        {
            output.push('.');
        }
    }
    if level == FormattingLevel::Aggressive && target != FormattingTarget::Code {
        output = output.replace("; ", ";\n");
        output = collapse_blank_lines(&output, 2);
    }
    TransformationOutcome {
        changed: output != text,
        text: output,
        warnings: Vec::new(),
    }
}

fn explicit_list(text: &str) -> String {
    let lower = text.to_lowercase();
    let list_prefix = ["lista: ", "lista, ", "lista "]
        .iter()
        .find(|prefix| lower.starts_with(**prefix));
    let Some(prefix) = list_prefix else {
        return text.to_string();
    };
    let body = &text[prefix.len()..];
    let ordinal_markers = [
        "primeiro item ",
        "segundo item ",
        "terceiro item ",
        "quarto item ",
        "first item ",
        "second item ",
        "third item ",
    ];
    let mut positions = ordinal_markers
        .iter()
        .flat_map(|marker| {
            find_all_case_insensitive(body, marker)
                .into_iter()
                .map(move |position| (position, marker.len()))
        })
        .collect::<Vec<_>>();
    positions.sort_unstable_by_key(|(position, _)| *position);
    positions.dedup_by_key(|(position, _)| *position);
    if positions.is_empty() || positions[0].0 != 0 {
        return text.to_string();
    }
    let mut items = Vec::new();
    for (index, (position, marker_len)) in positions.iter().enumerate() {
        let start = position + marker_len;
        let end = positions
            .get(index + 1)
            .map(|next| next.0)
            .unwrap_or(body.len());
        let item = body[start..end].trim().trim_matches([',', ';']);
        if !item.is_empty() {
            items.push(format!("- {item}"));
        }
    }
    if items.len() >= 2 {
        items.join("\n")
    } else {
        text.to_string()
    }
}

fn replace_spoken_command(text: &str, commands: &[&str], replacement: &str) -> String {
    let mut output = text.to_string();
    for command in commands {
        loop {
            let lower = output.to_lowercase();
            let Some(position) = lower.find(command) else {
                break;
            };
            output.replace_range(position..position + command.len(), replacement);
        }
    }
    output
}

fn find_all_case_insensitive(text: &str, needle: &str) -> Vec<usize> {
    let lower = text.to_lowercase();
    let mut positions = Vec::new();
    let mut offset = 0;
    while let Some(position) = lower[offset..].find(needle) {
        positions.push(offset + position);
        offset += position + needle.len();
    }
    positions
}

fn capitalize_sentence_start(text: &str) -> String {
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().collect::<String>() + characters.as_str()
}

fn collapse_blank_lines(text: &str, maximum: usize) -> String {
    let mut output = String::new();
    let mut consecutive = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            consecutive += 1;
            if consecutive > maximum {
                continue;
            }
        } else {
            consecutive = 0;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSpan {
    pub value: String,
    pub kind: &'static str,
}

pub fn protected_spans(text: &str) -> Vec<ProtectedSpan> {
    text.split_whitespace()
        .filter_map(|raw| {
            let value = raw.trim_matches(|character: char| {
                matches!(
                    character,
                    ',' | ';' | ':' | '(' | ')' | '[' | ']' | '"' | '\''
                )
            });
            let kind = classify_protected(value)?;
            Some(ProtectedSpan {
                value: value.to_string(),
                kind,
            })
        })
        .collect()
}

fn classify_protected(value: &str) -> Option<&'static str> {
    if value.starts_with("http://") || value.starts_with("https://") {
        return Some("url");
    }
    if value.starts_with("--") && value.len() > 2 {
        return Some("cli_flag");
    }
    if (value.contains("\\")
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../"))
        && value.len() > 2
    {
        return Some("path");
    }
    if value.rsplit_once('.').is_some_and(|(_, extension)| {
        (1..=10).contains(&extension.len())
            && extension
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
    }) {
        return Some("filename_or_version");
    }
    if value.starts_with('v')
        && value[1..].split('.').count() >= 2
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return Some("version");
    }
    if value.contains('_') || value.contains("::") || value.contains("->") {
        return Some("identifier");
    }
    if value
        .chars()
        .any(|character| character.is_ascii_lowercase())
        && value
            .chars()
            .any(|character| character.is_ascii_uppercase())
        && !value.contains(' ')
    {
        return Some("identifier");
    }
    if value.contains('/') && value.split('/').all(|part| !part.is_empty()) {
        return Some("model_or_provider_id");
    }
    None
}

pub fn enforce_protected_spans(source: &str, candidate: &str) -> TransformationOutcome {
    let missing = protected_spans(source)
        .into_iter()
        .filter(|span| !candidate.contains(&span.value))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return TransformationOutcome {
            text: candidate.to_string(),
            changed: source != candidate,
            warnings: Vec::new(),
        };
    }
    TransformationOutcome {
        text: source.to_string(),
        changed: false,
        warnings: vec![format!(
            "code_guard_rejected:{}",
            missing
                .iter()
                .map(|span| span.value.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )],
    }
}

fn looks_like_code_or_identifier(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.chars().any(char::is_whitespace)
        && protected_spans(trimmed).iter().any(|span| {
            span.value == trimmed
                && matches!(
                    span.kind,
                    "path" | "cli_flag" | "identifier" | "model_or_provider_id"
                )
        })
}

fn unchanged(text: &str) -> TransformationOutcome {
    TransformationOutcome {
        text: text.to_string(),
        changed: false,
        warnings: Vec::new(),
    }
}

fn changed(text: String, warning: &str) -> TransformationOutcome {
    TransformationOutcome {
        text,
        changed: true,
        warnings: vec![warning.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_backtrack_corpus() {
        for (input, expected) in [
            ("marca para duas... não, para três", "marca para três"),
            (
                "manda para o João... quer dizer, para a Maria",
                "manda para a Maria",
            ),
            ("usa Gemini... não, deixa no Whisper", "usa Whisper"),
        ] {
            assert_eq!(
                apply_backtrack(input, FormattingLevel::Smart).text,
                expected
            );
        }
    }

    #[test]
    fn literal_and_ambiguous_backtrack_are_preserved() {
        let input = "marca para duas... não, acho que depois";
        assert_eq!(apply_backtrack(input, FormattingLevel::Smart).text, input);
        assert_eq!(
            apply_backtrack("usa A... não, usa B", FormattingLevel::Literal).text,
            "usa A... não, usa B"
        );
    }

    #[test]
    fn formatting_levels_and_explicit_commands_are_distinct() {
        let input = "primeira linha nova linha segunda linha";
        assert_eq!(
            apply_smart_formatting(input, FormattingLevel::Literal, FormattingTarget::PlainText)
                .text,
            "primeira linha \n segunda linha"
        );
        assert_eq!(
            apply_smart_formatting(
                "olá mundo",
                FormattingLevel::Smart,
                FormattingTarget::PlainText
            )
            .text,
            "Olá mundo."
        );
        assert_eq!(
            apply_smart_formatting(
                "um; dois",
                FormattingLevel::Aggressive,
                FormattingTarget::PlainText
            )
            .text,
            "Um;\ndois."
        );
    }

    #[test]
    fn explicit_markdown_list_requires_markdown_target() {
        let spoken = "lista primeiro item Alpha segundo item Beta";
        assert_eq!(
            apply_smart_formatting(spoken, FormattingLevel::Smart, FormattingTarget::Markdown).text,
            "- Alpha\n- Beta."
        );
        assert!(!apply_smart_formatting(
            spoken,
            FormattingLevel::Smart,
            FormattingTarget::PlainText
        )
        .text
        .starts_with('-'));
    }

    #[test]
    fn code_guard_rejects_changes_to_protected_spans() {
        for (source, broken) in [
            ("abra provider-routing.json", "abra provider routing.json"),
            ("rode --no-cache", "rode no cache"),
            ("use C:\\Dev\\app.rs", "use C Dev app.rs"),
            ("acesse https://example.com/a", "acesse example.com/a"),
            ("instale v1.2.3", "instale 1.2.3"),
            ("chame foo_bar::run", "chame foo bar run"),
        ] {
            assert_eq!(enforce_protected_spans(source, broken).text, source);
        }
    }
}
