//! File name construction.
//!
//! Two jobs: expand the user's template, and make the result something Windows
//! will actually accept. Titles arrive from third-party sources, so they can
//! contain path separators, reserved device names, emoji, RTL marks and a few
//! hundred characters of clickbait -- all of which are handled here rather than
//! anywhere near a shell.

use std::path::{Path, PathBuf};

/// Characters Windows rejects outright in a file name.
const INVALID: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Device names that cannot be used as a file stem on Windows, with or without
/// an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Leaves room inside the classic 260-char path limit for the directory, the
/// extension, a " (n)" disambiguator and the trailing NUL.
const MAX_STEM_CHARS: usize = 120;

/// Strip everything a file system would refuse, then tidy the result.
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());

    for ch in input.chars() {
        if INVALID.contains(&ch) {
            out.push('-');
        } else if (ch as u32) < 0x20 || ch == '\u{7f}' {
            // Control characters: drop silently.
            continue;
        } else if matches!(ch, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}') {
            // Bidirectional overrides can make a name display as something
            // other than what it is. Not useful in a file name.
            continue;
        } else {
            out.push(ch);
        }
    }

    // Collapse runs of whitespace, then trim what Windows would silently strip
    // from the end (trailing dots and spaces are not preserved).
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace());

    if trimmed.is_empty() {
        return String::new();
    }

    let upper = trimmed.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or(&upper);
    if RESERVED.contains(&stem) {
        return format!("_{trimmed}");
    }

    trimmed.to_string()
}

/// Truncate on a character boundary, not a byte one, and avoid ending on a
/// partial word when there is a nearby space to cut at.
pub fn truncate_stem(stem: &str, max_chars: usize) -> String {
    if stem.chars().count() <= max_chars {
        return stem.to_string();
    }

    let truncated: String = stem.chars().take(max_chars).collect();
    let cut = truncated
        .rfind(' ')
        .filter(|index| *index > max_chars * 2 / 3)
        .unwrap_or(truncated.len());

    truncated[..cut].trim_end_matches(['.', ' ', '-']).to_string()
}

pub struct NameContext<'a> {
    pub title: &'a str,
    pub creator: Option<&'a str>,
    pub quality: &'a str,
    pub platform: &'a str,
    /// ISO date, `YYYY-MM-DD`.
    pub date: &'a str,
    pub ext: &'a str,
}

/// Expand `{token}` placeholders. Unknown tokens are left as-is so a typo is
/// visible in the preview rather than silently swallowed.
pub fn render_template(template: &str, ctx: &NameContext<'_>) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return post_process(out, ctx);
        };

        let token = &after[..end];
        match token {
            "title" => out.push_str(ctx.title),
            "creator" => out.push_str(ctx.creator.unwrap_or("")),
            "quality" => out.push_str(ctx.quality),
            "platform" => out.push_str(ctx.platform),
            "date" => out.push_str(ctx.date),
            "ext" => out.push_str(ctx.ext),
            unknown => {
                out.push('{');
                out.push_str(unknown);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }

    out.push_str(rest);
    post_process(out, ctx)
}

/// Clean up the seams left when an optional token expanded to nothing, e.g.
/// "{creator} - {title}" with no creator would otherwise start with " - ".
fn post_process(rendered: String, ctx: &NameContext<'_>) -> String {
    let mut value = rendered;
    for separator in [" - ", " – ", " | ", " · "] {
        while value.starts_with(separator) {
            value = value[separator.len()..].to_string();
        }
        while value.ends_with(separator) {
            value.truncate(value.len() - separator.len());
        }
        // Collapse a doubled separator left by a middle token vanishing.
        let doubled = format!("{separator}{separator}");
        while value.contains(&doubled) {
            value = value.replace(&doubled, separator);
        }
    }
    value = value.replace("[]", "").replace("()", "");

    let cleaned = sanitize_component(&value);
    if cleaned.is_empty() {
        sanitize_component(ctx.title)
    } else {
        cleaned
    }
}

/// Build the final path, expanding the template, sanitising it, keeping it
/// inside the length limit and making it unique in the target directory.
pub fn build_output_path(
    dir: &Path,
    template: &str,
    ctx: &NameContext<'_>,
) -> PathBuf {
    let mut stem = render_template(template, ctx);
    if stem.is_empty() {
        stem = "download".to_string();
    }

    // Leave headroom so `dir + stem + ext` stays inside the legacy path limit
    // even on a deeply nested download folder.
    let dir_len = dir.as_os_str().len();
    let budget = MAX_STEM_CHARS.min(240usize.saturating_sub(dir_len + ctx.ext.len() + 8));
    stem = truncate_stem(&stem, budget.max(16));

    let ext = ctx.ext.trim_start_matches('.');
    unique_path(dir, &stem, ext)
}

/// `name.mp4`, then `name (2).mp4`, `name (3).mp4`, ...
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    });
    if !first.exists() {
        return first;
    }

    for index in 2..10_000u32 {
        let candidate = dir.join(if ext.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{ext}")
        });
        if !candidate.exists() {
            return candidate;
        }
    }

    // Pathological case: fall back to a timestamped name rather than looping.
    dir.join(format!("{stem} ({}).{ext}", crate::util::now_ms()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>() -> NameContext<'a> {
        NameContext {
            title: "My Video",
            creator: Some("creator"),
            quality: "1080p",
            platform: "youtube",
            date: "2026-09-07",
            ext: "mp4",
        }
    }

    #[test]
    fn strips_windows_reserved_characters() {
        assert_eq!(sanitize_component(r#"a/b\c:d*e?f"g<h>i|j"#), "a-b-c-d-e-f-g-h-i-j");
    }

    #[test]
    fn drops_control_and_bidi_characters() {
        assert_eq!(sanitize_component("clean\u{202e}nes\u{0007}s"), "cleanness");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_component("name...  "), "name");
        assert_eq!(sanitize_component("  spaced  out  "), "spaced out");
    }

    #[test]
    fn escapes_reserved_device_names() {
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_component("console"), "console");
    }

    #[test]
    fn empty_after_sanitizing_returns_empty() {
        assert_eq!(sanitize_component("..."), "");
        assert_eq!(sanitize_component("\u{0001}"), "");
    }

    #[test]
    fn renders_all_tokens() {
        let rendered = render_template("{creator} - {title} [{quality}] {platform} {date}", &ctx());
        assert_eq!(rendered, "creator - My Video [1080p] youtube 2026-09-07");
    }

    #[test]
    fn collapses_separators_around_a_missing_token() {
        let mut context = ctx();
        context.creator = None;
        assert_eq!(render_template("{creator} - {title}", &context), "My Video");
    }

    #[test]
    fn keeps_unknown_tokens_visible() {
        assert_eq!(render_template("{title} {bogus}", &ctx()), "My Video {bogus}");
    }

    #[test]
    fn unterminated_token_is_left_alone() {
        assert_eq!(render_template("{title} {oops", &ctx()), "My Video {oops");
    }

    #[test]
    fn falls_back_to_the_title_when_a_template_renders_empty() {
        assert_eq!(render_template("{creator}", &{
            let mut c = ctx();
            c.creator = None;
            c
        }), "My Video");
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let long = "ü".repeat(400);
        let cut = truncate_stem(&long, 50);
        assert_eq!(cut.chars().count(), 50);
    }

    #[test]
    fn truncation_prefers_a_word_boundary() {
        let text = "one two three four five six seven eight nine ten eleven";
        let cut = truncate_stem(text, 30);
        assert!(!cut.ends_with(' '));
        assert!(cut.len() <= 30);
        assert!(text.starts_with(&cut));
    }

    #[test]
    fn short_names_are_not_truncated() {
        assert_eq!(truncate_stem("short", 100), "short");
    }
}
