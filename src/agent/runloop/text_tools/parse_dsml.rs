use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::parser::{ParseResult, ParsedToolCall, TextualToolParser};

const DSML_MARKER: &str = "DSML";

#[derive(Debug, Clone, Copy)]
struct DsmlTag<'a> {
    start: usize,
    end: usize,
    closing: bool,
    name: &'a str,
    attributes: &'a str,
}

fn is_dsml_bar(ch: char) -> bool {
    matches!(ch, '\u{ff5c}' | '|')
}

fn skip_dsml_whitespace(text: &str, mut cursor: usize) -> usize {
    while let Some(ch) = text[cursor..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn consume_dsml_bars(text: &str, mut cursor: usize) -> Option<usize> {
    let mut count = 0;
    loop {
        cursor = skip_dsml_whitespace(text, cursor);
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };
        if !is_dsml_bar(ch) {
            break;
        }
        count += 1;
        cursor += ch.len_utf8();
    }

    (count > 0).then_some(cursor)
}

fn parse_dsml_tag_at(text: &str, start: usize) -> Option<DsmlTag<'_>> {
    if !text.get(start..)?.starts_with('<') {
        return None;
    }

    let mut cursor = start + '<'.len_utf8();
    cursor = skip_dsml_whitespace(text, cursor);
    let closing = text[cursor..].starts_with('/');
    if closing {
        cursor += '/'.len_utf8();
    }

    cursor = consume_dsml_bars(text, cursor)?;
    cursor = skip_dsml_whitespace(text, cursor);
    let marker_end = cursor.checked_add(DSML_MARKER.len())?;
    if !text.get(cursor..marker_end)?.eq_ignore_ascii_case(DSML_MARKER) {
        return None;
    }
    cursor = consume_dsml_bars(text, marker_end)?;
    cursor = skip_dsml_whitespace(text, cursor);

    let name_start = cursor;
    while let Some(ch) = text[cursor..].chars().next() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            cursor += ch.len_utf8();
        } else {
            break;
        }
    }
    if name_start == cursor {
        return None;
    }

    let name = &text[name_start..cursor];
    let attributes_start = cursor;
    let close_offset = text[attributes_start..].find('>')?;
    let close_start = attributes_start + close_offset;

    Some(DsmlTag {
        start,
        end: close_start + '>'.len_utf8(),
        closing,
        name,
        attributes: &text[attributes_start..close_start],
    })
}

fn find_dsml_tag<'a>(text: &'a str, from: usize, name: &str, closing: bool) -> Option<DsmlTag<'a>> {
    let mut search_start = from;
    while let Some(relative_start) = text[search_start..].find('<') {
        let start = search_start + relative_start;
        if let Some(tag) = parse_dsml_tag_at(text, start)
            && tag.closing == closing
            && tag.name.eq_ignore_ascii_case(name)
        {
            return Some(tag);
        }
        search_start = start + '<'.len_utf8();
    }
    None
}

fn find_dsml_attribute<'a>(attributes: &'a str, key: &str) -> Option<&'a str> {
    let mut search_start = 0;
    while let Some(relative_start) = attributes[search_start..].find(key) {
        let key_start = search_start + relative_start;
        let has_identifier_prefix = attributes[..key_start]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
        if has_identifier_prefix {
            search_start = key_start + key.len();
            continue;
        }

        let mut cursor = skip_dsml_whitespace(attributes, key_start + key.len());
        if !attributes[cursor..].starts_with('=') {
            search_start = key_start + key.len();
            continue;
        }
        cursor += '='.len_utf8();
        cursor = skip_dsml_whitespace(attributes, cursor);
        let quote = attributes[cursor..].chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let value_start = cursor + quote.len_utf8();
        let value_end = attributes[value_start..].find(quote)?;
        return Some(&attributes[value_start..value_start + value_end]);
    }
    None
}

/// Strips DSML markup from text, including tokenized variants with whitespace
/// around the separator bars, while preserving non-tag content (including
/// parameter values).
pub(crate) fn strip_dsml_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < text.len() {
        if text.as_bytes()[cursor] == b'<'
            && let Some(tag) = parse_dsml_tag_at(text, cursor)
        {
            cursor = tag.end;
            continue;
        }

        if let Some(ch) = text[cursor..].chars().next() {
            out.push(ch);
            cursor += ch.len_utf8();
        }
    }

    out
}

pub(crate) fn contains_dsml_markup(text: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative_start) = text[search_start..].find('<') {
        let start = search_start + relative_start;
        if parse_dsml_tag_at(text, start).is_some() {
            return true;
        }
        search_start = start + '<'.len_utf8();
    }
    false
}

/// Public wrapper for tests
#[cfg(test)]
fn parse_dsml_tool_call(text: &str) -> Option<(String, Value)> {
    parse_dsml_tool_call_raw(text)
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
fn parse_dsml_tool_call_raw(text: &str) -> Option<(String, Value)> {
    let invoke = find_dsml_tag(text, 0, "invoke", false)?;
    let name = find_dsml_attribute(invoke.attributes, "name")?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let invoke_close = find_dsml_tag(text, invoke.end, "invoke", true)?;
    let content = &text[invoke.end..invoke_close.start];

    let mut object = Map::new();
    let mut search_start = 0;

    while let Some(param) = find_dsml_tag(content, search_start, "parameter", false) {
        let param_close = find_dsml_tag(content, param.end, "parameter", true)?;
        let param_name = find_dsml_attribute(param.attributes, "name")?.trim().to_string();
        let is_string =
            find_dsml_attribute(param.attributes, "string").is_some_and(|value| value.eq_ignore_ascii_case("true"));
        let raw_value = content[param.end..param_close.start].trim();

        let value = if is_string {
            Value::String(raw_value.to_string())
        } else {
            serde_json::from_str::<Value>(raw_value).unwrap_or_else(|_| Value::String(raw_value.to_string()))
        };

        object.insert(param_name, value);
        search_start = param_close.end;
    }

    if object.is_empty() {
        return None;
    }

    Some((name, Value::Object(object)))
}

/// Collects DSML invoke regions for stripping.
pub(super) fn collect_dsml_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0;
    while let Some(invoke) = find_dsml_tag(text, search_start, "invoke", false) {
        let end = find_dsml_tag(text, invoke.end, "invoke", true).map_or(text.len(), |close| close.end);
        if invoke.start < end {
            regions.push((invoke.start, end));
        }
        search_start = end.max(invoke.end);
    }
}

/// Parser for DeepSeek DSML v2 format tool calls.
pub(crate) struct DsmlToolParser;

impl TextualToolParser for DsmlToolParser {
    fn name(&self) -> &'static str {
        "dsml"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
        match parse_dsml_tool_call_raw(text) {
            Some((name, args)) => ParseResult::Success(ParsedToolCall { name, args }),
            None => {
                tracing::debug!(parser = "dsml", reason = "no matching DSML v2 pattern", "Rejected textual tool call");
                ParseResult::Reject("no matching DSML v2 pattern")
            }
        }
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        collect_dsml_regions(text, &mut regions);
        regions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parses_single_dsml_invoke_with_string_params() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"query\" string=\"true\">Widget</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/src</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"result_types\">[\"definition\"]</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["query"], Value::String("Widget".to_string()));
        assert_eq!(args["path"], Value::String("/src".to_string()));
        assert_eq!(args["result_types"], serde_json::json!(["definition"]));
    }

    #[test]
    fn parses_dsml_invoke_inside_tool_calls_wrapper() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "    <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "  </\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "read_file");
        assert_eq!(args["path"], Value::String("/tmp/foo.txt".to_string()));
    }

    #[test]
    fn parses_dsml_with_whitespace_between_special_token_bars() {
        let text = concat!(
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"exec_command\">\n",
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter name=\"cmd\" string=\"true\">true</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter>\n",
            "</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("spaced DSML should parse");
        assert_eq!(name, "exec_command");
        assert_eq!(args["cmd"], Value::String("true".to_string()));
        assert!(contains_dsml_markup(text));
    }

    #[test]
    fn parses_first_invoke_only_when_multiple_present() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"a\" string=\"true\">1</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "  <\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"b\" string=\"true\">2</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["a"], Value::String("1".to_string()));
        assert!(args.get("b").is_none());
    }

    #[test]
    fn returns_none_for_non_dsml_text() {
        assert!(parse_dsml_tool_call("plain text without any dsml tags").is_none());
    }

    #[test]
    fn handles_json_value_params_without_string_true() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"code_search\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"count\">42</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        let (name, args) = parse_dsml_tool_call(text).expect("should parse");
        assert_eq!(name, "code_search");
        assert_eq!(args["count"], Value::Number(serde_json::Number::from(42)));
    }

    #[test]
    fn returns_none_for_empty_invoke_name() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"\">\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>",
        );

        assert!(parse_dsml_tool_call(text).is_none());
    }

    // --- strip_dsml_markup tests ---

    #[test]
    fn strip_dsml_preserves_plain_text() {
        let input = "This is plain text without any DSML tags.";
        let result = strip_dsml_markup(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_dsml_removes_single_invoke_with_params() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"task_tracker\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"action\" string=\"true\">update</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"item_index\" string=\"false\">1</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );
        let result = strip_dsml_markup(text);
        assert!(!result.contains("DSML"));
        assert!(!result.contains("\u{ff5c}"));
    }

    #[test]
    fn strip_dsml_preserves_text_around_tags() {
        let text = concat!(
            "Here is my synthesis.\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "The key finding is...",
        );
        let result = strip_dsml_markup(text);
        assert!(result.contains("Here is my synthesis."));
        assert!(result.contains("The key finding is..."));
        assert!(!result.contains("DSML"));
    }

    #[test]
    fn strip_dsml_removes_spaced_special_token_tags() {
        let text = concat!(
            "Before.\n",
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"read_file\">\n",
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter name=\"path\" string=\"true\">README.md</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter>\n",
            "</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>\n",
            "After.",
        );

        let result = strip_dsml_markup(text);
        assert!(result.contains("Before."));
        assert!(result.contains("README.md"));
        assert!(result.contains("After."));
        assert!(!result.contains("DSML"));
        assert!(!contains_dsml_markup(&result));
    }

    #[test]
    fn strip_dsml_empty_for_pure_tags() {
        let text = concat!(
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n",
            "<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">/tmp/foo.txt</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n",
            "</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>",
        );
        let result = strip_dsml_markup(text);
        let trimmed = result.trim();
        assert!(trimmed.is_empty() || !trimmed.contains("DSML"));
    }
}
