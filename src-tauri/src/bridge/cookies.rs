//! The Netscape `cookies.txt` format, which is what the engine reads.
//!
//! The format is decades older than this app and still exactly what yt-dlp
//! wants: seven tab-separated columns per line -- domain, include-subdomains,
//! path, secure, expiry, name, value -- with `TRUE`/`FALSE` in the two flag
//! columns and an expiry of `0` for a cookie that dies with the browser
//! session. A leading dot on the domain is what makes the include-subdomains
//! column true; nothing else carries that meaning.
//!
//! Two details are not in the original file format but are universal in
//! practice, and both are what yt-dlp itself writes: the `# Netscape HTTP
//! Cookie File` first line, without which most parsers reject the file before
//! reading a single cookie, and the `#HttpOnly_` domain prefix, which is how a
//! line that looks like a comment carries the httpOnly flag.
//!
//! Reading matters as much as writing. yt-dlp rewrites the file it was given
//! when it exits, with whatever the server rotated during the run, and `read`
//! is how that rotation finds its way back into the stored session.

use crate::bridge::protocol::Cookie;

const HEADER: &str =
    "# Netscape HTTP Cookie File\n# Written by Universal Downloader for a single download.\n\n";

/// The prefix yt-dlp and curl both use to mark an httpOnly cookie.
const HTTP_ONLY_PREFIX: &str = "#HttpOnly_";

pub fn write(cookies: &[Cookie]) -> String {
    let mut out = String::from(HEADER);

    for cookie in cookies {
        if !representable(cookie) {
            continue;
        }

        // The prefix goes in front of the whole domain, dot included, so a
        // subdomain-wide httpOnly cookie reads `#HttpOnly_.youtube.com`.
        let domain = if cookie.http_only {
            format!("{HTTP_ONLY_PREFIX}{}", cookie.domain)
        } else {
            cookie.domain.clone()
        };
        // Chrome always reports a path, but an empty one would produce a file
        // the engine reads as malformed rather than as "the whole site".
        let path = if cookie.path.is_empty() {
            "/"
        } else {
            cookie.path.as_str()
        };

        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            domain,
            flag(cookie.domain.starts_with('.')),
            path,
            flag(cookie.secure),
            expiry(cookie.expiration_date),
            cookie.name,
            cookie.value,
        ));
    }

    out
}

pub fn read(text: &str) -> Vec<Cookie> {
    let mut cookies = Vec::new();

    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');

        let (line, http_only) = match line.strip_prefix(HTTP_ONLY_PREFIX) {
            Some(rest) => (rest, true),
            None => (line, false),
        };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Seven at most, never fewer: splitting no further than the value
        // column keeps a value that somehow contains a tab in one piece
        // instead of turning the line into an eighth field and losing it.
        let fields: Vec<&str> = line.splitn(7, '\t').collect();
        if fields.len() != 7 {
            continue;
        }

        let expires: f64 = fields[4].trim().parse().unwrap_or(0.0);

        cookies.push(Cookie {
            domain: fields[0].to_string(),
            name: fields[5].to_string(),
            value: fields[6].to_string(),
            path: fields[2].to_string(),
            secure: is_true(fields[3]),
            http_only,
            // Column two says the same thing as the leading dot and is
            // redundant, so the domain text is kept verbatim and the column is
            // ignored -- writing it back out reproduces it either way.
            expiration_date: (expires > 0.0).then_some(expires),
        });
    }

    cookies
}

/// A tab or a newline anywhere in a cookie would not survive the round trip:
/// it would either split a line into the wrong number of columns or end it
/// early. Dropping such a cookie loses one entry; writing it loses the file.
fn representable(cookie: &Cookie) -> bool {
    let fields = [
        cookie.domain.as_str(),
        cookie.path.as_str(),
        cookie.name.as_str(),
        cookie.value.as_str(),
    ];
    !fields
        .iter()
        .any(|field| field.contains(['\t', '\n', '\r']))
}

fn flag(value: bool) -> &'static str {
    if value {
        "TRUE"
    } else {
        "FALSE"
    }
}

/// Whole seconds since the epoch, and `0` for a session cookie -- which is what
/// yt-dlp reads back as "discard this when the run ends".
fn expiry(expiration_date: Option<f64>) -> i64 {
    expiration_date
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| seconds as i64)
        .unwrap_or(0)
}

fn is_true(field: &str) -> bool {
    field.trim().eq_ignore_ascii_case("TRUE")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(domain: &str, name: &str, value: &str) -> Cookie {
        Cookie {
            domain: domain.to_string(),
            name: name.to_string(),
            value: value.to_string(),
            path: "/".to_string(),
            secure: true,
            http_only: false,
            expiration_date: Some(1_900_000_000.0),
        }
    }

    #[test]
    fn the_header_is_the_one_the_engine_looks_for() {
        assert!(write(&[]).starts_with("# Netscape HTTP Cookie File\n"));
    }

    #[test]
    fn a_line_has_seven_tab_separated_columns() {
        let text = write(&[cookie(".youtube.com", "SID", "abc")]);
        let line = text.lines().find(|line| line.contains("SID")).unwrap();
        assert_eq!(line.split('\t').count(), 7);
        assert_eq!(line, ".youtube.com\tTRUE\t/\tTRUE\t1900000000\tSID\tabc");
    }

    #[test]
    fn a_leading_dot_is_what_sets_the_subdomain_column() {
        let with_dot = write(&[cookie(".youtube.com", "SID", "abc")]);
        let without = write(&[cookie("youtube.com", "SID", "abc")]);
        assert!(with_dot.contains(".youtube.com\tTRUE\t"));
        assert!(without.contains("youtube.com\tFALSE\t"));
    }

    #[test]
    fn a_session_cookie_is_written_with_a_zero_expiry() {
        let mut session = cookie(".youtube.com", "YSC", "temporary");
        session.expiration_date = None;
        let text = write(&[session]);
        assert!(text.contains("\t0\tYSC\t"), "{text}");
        assert_eq!(read(&text)[0].expiration_date, None);
    }

    #[test]
    fn an_http_only_cookie_survives_the_prefix() {
        let mut secret = cookie(".youtube.com", "__Secure-1PSID", "value");
        secret.http_only = true;
        let text = write(&[secret]);
        assert!(text.contains("#HttpOnly_.youtube.com\t"), "{text}");

        let back = read(&text);
        assert_eq!(back.len(), 1);
        assert!(back[0].http_only);
        assert_eq!(back[0].domain, ".youtube.com");
        assert_eq!(back[0].name, "__Secure-1PSID");
    }

    #[test]
    fn a_value_with_a_space_is_not_split() {
        let text = write(&[cookie(".youtube.com", "PREF", "tz=Europe Istanbul")]);
        let back = read(&text);
        assert_eq!(back[0].value, "tz=Europe Istanbul");
    }

    #[test]
    fn a_round_trip_keeps_every_field() {
        let mut http_only = cookie(".youtube.com", "LOGIN_INFO", "one two three");
        http_only.http_only = true;
        http_only.secure = true;

        let mut session = cookie("youtube.com", "YSC", "short lived");
        session.expiration_date = None;
        session.secure = false;
        session.path = "/watch".to_string();

        let original = vec![http_only, session, cookie(".google.com", "SAPISID", "v")];
        let back = read(&write(&original));

        assert_eq!(back.len(), original.len());
        for (before, after) in original.iter().zip(back.iter()) {
            assert_eq!(before.domain, after.domain);
            assert_eq!(before.name, after.name);
            assert_eq!(before.value, after.value);
            assert_eq!(before.path, after.path);
            assert_eq!(before.secure, after.secure);
            assert_eq!(before.http_only, after.http_only);
            assert_eq!(before.expiration_date, after.expiration_date);
        }
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text =
            "# Netscape HTTP Cookie File\n\n# a note\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tv\n";
        let back = read(text);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "SID");
    }

    #[test]
    fn a_carriage_return_does_not_end_up_in_the_value() {
        let back = read(".youtube.com\tTRUE\t/\tTRUE\t0\tSID\tv\r\n");
        assert_eq!(back[0].value, "v");
    }

    #[test]
    fn a_cookie_with_a_tab_in_it_is_dropped_rather_than_written_broken() {
        let text = write(&[cookie(".youtube.com", "SID", "a\tb")]);
        assert_eq!(read(&text).len(), 0);
    }
}
