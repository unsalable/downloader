//! Comparing titles written by different people.
//!
//! A song's name on Spotify and the title of its upload, or an anime's name
//! as typed and as its episodes are titled, agree in their words and little
//! else: case, accents and punctuation all differ. Both sides are reduced to
//! plain words first.

/// Lowercase, accents folded, punctuation turned into spaces, so that
/// "Şımarık" and "Simarik" and "ADÈLA" and "Adela" compare equal.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        let folded: &str = match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'ç' | 'ć' | 'č' => "c",
            'ď' | 'đ' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'ł' => "l",
            'ñ' | 'ń' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'ŕ' | 'ř' => "r",
            'ś' | 'š' | 'ş' => "s",
            'ť' | 'ţ' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            '&' => " and ",
            _ if ch.is_alphanumeric() => {
                out.push(ch);
                continue;
            }
            _ => " ",
        };
        out.push_str(folded);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `needle`'s words appear, in order, in `haystack`. Word-wise, so
/// "live" is not found in "alive".
pub fn contains_words(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    format!(" {haystack} ").contains(&format!(" {needle} "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_case_and_punctuation_do_not_count() {
        assert_eq!(normalize("TARKAN - Şımarık!"), "tarkan simarik");
        assert_eq!(normalize("ADÈLA & Friends"), "adela and friends");
        assert_eq!(normalize("  "), "");
    }

    #[test]
    fn words_are_found_whole() {
        assert!(contains_words("blinding lights live", "live"));
        assert!(!contains_words("stayin alive", "live"));
        assert!(contains_words("the weeknd blinding lights", "blinding lights"));
        assert!(!contains_words("anything", ""));
    }
}
