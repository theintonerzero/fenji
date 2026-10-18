//! The markup in the game's text (retranslation spec §5):
//! - bracket tags, like BBCode;
//! - brace placeholders, in Python's format style, and printf's `%s`;
//! - HTML-like tags and entities;
//! - the codes of skill text between pipes, and pairs of hashes that highlight;
//! - line breaks.
//!
//! Brackets around anything but a known tag are punctuation, not markup.
//!
//! A token's visible text, which players read and the translation translates, is a slot in its
//! shape (`\u{1}`). Slots are found by the token's structure, never by what language they hold,
//! so that a token and its translation have the same shape.

/// A family of markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Bracket,
    Brace,
    Html,
    Code,
    LineBreak,
}

/// One token of markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub family: Family,
    /// The token as written, each visible text replaced by `\u{1}`.
    pub shape: String,
    /// Its visible texts, in order.
    pub visible: Vec<String>,
}

/// A slot in a shape.
pub const SLOT: char = '\u{1}';

/// The bracket tags the game's text uses, from the shapes of every source line (§5; Task 4,
/// step 6). Brackets around anything else, such as a word, a key's name (`[W]`, `[Ctrl]`) or a
/// placeholder (`[{2}]`), are punctuation, as today's English treats them.
pub const BRACKET_TAGS: &[&str] = &[
    "autovisible",
    "b",
    "bimg",
    "br",
    "btn",
    "btt",
    "buff",
    "c",
    "camp",
    "center",
    "color",
    "combatprop",
    "d_npc",
    "d_perform",
    "d_player",
    "date",
    "duration",
    "emoji",
    "font",
    "functionkey",
    "gender",
    "glow",
    "i",
    "img",
    "indent",
    "item",
    "last_time",
    "leading",
    "left",
    "link",
    "localnum",
    "m",
    "money",
    "panel",
    "pskill",
    "q_bgm",
    "q_guid",
    "q_npcname",
    "q_roletalk",
    "q_sound",
    "right",
    "rolename",
    "size",
    "skill",
    "space",
    "strike",
    "u",
    "vocation",
];

/// Each tag's attributes whose values players read, `*` for all of them: slots (Task 4, step 6).
/// Every other value, such as a colour, an id, a sound or an action, stays exactly as it is, even
/// one called `name` (`[functionkey name="ui.map"]`, `[money name='Gold']`).
pub const TEXT_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("btn", &["text"]),
    ("btt", &["text"]),
    ("camp", &["*"]),
    ("d_npc", &["name", "nickname"]),
    ("gender", &["male", "female"]),
    ("link", &["text"]),
    ("m", &["text"]),
    ("panel", &["title"]),
    ("q_npcname", &["text"]),
    ("vocation", &["*"]),
    // An HTML tag's, by its lowercase name.
    ("w", &["button"]),
];

/// Whether `tag`'s attribute `name` holds text players read.
fn text_attribute(tag: &str, name: &str) -> bool {
    TEXT_ATTRIBUTES
        .iter()
        .any(|(t, names)| *t == tag && (names.contains(&"*") || names.contains(&name)))
}

/// The longest token, in characters: past it, an opening `[`, `{`, `<`, `|` or `#` is text.
const MAX_TOKEN: usize = 200;
/// The longest code between pipes or hashes.
const MAX_CODE: usize = 60;

/// One token found, with where it starts and ends in the text's characters.
struct Found {
    token: Token,
    start: usize,
    end: usize,
}

/// `chars[i]` opens a token that ends at the first `close` before `MAX_TOKEN`, with no `open`
/// or line break between: the content's end, exclusive.
fn closing(chars: &[char], i: usize, open: char, close: char, max: usize) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(i + 1)
        .take(max)
        .find(|&(_, &c)| c == close || c == open || c == '\n')
        .filter(|&(_, &c)| c == close)
        .map(|(j, _)| j)
}

/// `content`'s quoted values, `tag`'s text attributes' made slots: the new content and its slots.
fn attributes(tag: &str, content: &str) -> (String, Vec<String>) {
    let mut shape = String::new();
    let mut visible = Vec::new();
    let mut rest = content;
    while let Some(q) = rest.find(['"', '\'']) {
        let quote = rest[q..].chars().next().unwrap_or('"');
        let before = &rest[..q];
        let Some(len) = rest[q + 1..].find(quote) else {
            break;
        };
        let value = &rest[q + 1..q + 1 + len];
        let name = before
            .trim_end()
            .trim_end_matches('=')
            .trim_end()
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("");
        shape.push_str(before);
        shape.push(quote);
        if text_attribute(tag, name) && !value.is_empty() {
            shape.push(SLOT);
            visible.push(value.to_string());
        } else {
            shape.push_str(value);
        }
        shape.push(quote);
        rest = &rest[q + 1 + len + 1..];
    }
    shape.push_str(rest);
    (shape, visible)
}

fn token(family: Family, shape: String, visible: Vec<String>) -> Token {
    Token {
        family,
        shape,
        visible,
    }
}

fn bracket(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let j = closing(chars, i, '[', ']', MAX_TOKEN)?;
    let content: String = chars[i + 1..j].iter().collect();
    if content.trim().is_empty() {
        return None;
    }
    let name: String = content
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    // Brackets around anything but a tag are punctuation: a word, a key's name, a placeholder.
    if !BRACKET_TAGS.contains(&name.as_str()) {
        return None;
    }
    let t = if content.starts_with('/') {
        token(Family::Bracket, format!("[{content}]"), vec![])
    } else {
        let (shape, visible) = attributes(&name, &content);
        token(Family::Bracket, format!("[{shape}]"), visible)
    };
    Some((t, j + 1 - i))
}

fn brace(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let j = closing(chars, i, '{', '}', MAX_TOKEN)?;
    let content: String = chars[i + 1..j].iter().collect();
    if !content.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    // `{color <hex> <text>}`: its text is drawn in the colour.
    let mut parts = content.splitn(3, ' ');
    let t = match (parts.next(), parts.next(), parts.next()) {
        (Some("color"), Some(hex), Some(text)) if !text.is_empty() => token(
            Family::Brace,
            format!("{{color {hex} {SLOT}}}"),
            vec![text.to_string()],
        ),
        _ => token(Family::Brace, format!("{{{content}}}"), vec![]),
    };
    Some((t, j + 1 - i))
}

fn html(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let after = if chars.get(i + 1) == Some(&'/') {
        i + 2
    } else {
        i + 1
    };
    if !chars.get(after).is_some_and(char::is_ascii_alphabetic) {
        return None;
    }
    let j = closing(chars, i, '<', '>', MAX_TOKEN)?;
    let content: String = chars[i + 1..j].iter().collect();
    let name: String = chars[after..j]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let (shape, visible) = attributes(&name, &content);
    Some((
        token(Family::Html, format!("<{shape}>"), visible),
        j + 1 - i,
    ))
}

/// `&nbsp;` and its kin.
fn entity(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let name: String = chars[i + 1..]
        .iter()
        .take(10)
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '#')
        .collect();
    let len = name.chars().count();
    (len > 0 && chars.get(i + 1 + len) == Some(&';'))
        .then(|| (token(Family::Html, format!("&{name};"), vec![]), len + 2))
}

/// printf's placeholders: `%s`, `%d`, `%5.2f` and the like.
fn printf(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let mut j = i + 1;
    while chars
        .get(j)
        .is_some_and(|c| c.is_ascii_digit() || *c == '.')
    {
        j += 1;
    }
    let conversion = *chars.get(j)?;
    if !"sdfiuxc".contains(conversion) {
        return None;
    }
    let raw: String = chars[i..=j].iter().collect();
    Some((token(Family::Brace, raw, vec![]), j + 1 - i))
}

/// The end of a pair of `delimiter`s opened at `i`: the next one on the same line, at most
/// [`MAX_CODE`] on, around content that neither starts nor ends with white space.
fn paired(chars: &[char], i: usize, delimiter: char) -> Option<usize> {
    let j = chars
        .iter()
        .enumerate()
        .skip(i + 1)
        .take(MAX_CODE)
        .find(|&(_, &c)| c == delimiter || c == '\n')
        .filter(|&(_, &c)| c == delimiter)
        .map(|(j, _)| j)?;
    let content = &chars[i + 1..j];
    let edge = |c: Option<&char>| c.is_some_and(|c| c.is_whitespace());
    (!content.is_empty() && !edge(content.first()) && !edge(content.last())).then_some(j)
}

/// A code between pipes. One whose content is a formula, a lowercase name and its arguments such
/// as `cp,1,2` or `cp,1,~`, stays as it is; any other content, such as a word, is visible.
fn code(chars: &[char], i: usize) -> Option<(Token, usize)> {
    let j = paired(chars, i, '|')?;
    let content: String = chars[i + 1..j].iter().collect();
    let formula = content.starts_with(|c: char| c.is_ascii_lowercase())
        && content
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_,.-+*/%~".contains(c));
    let t = if formula {
        token(Family::Code, format!("|{content}|"), vec![])
    } else {
        token(Family::Code, format!("|{SLOT}|"), vec![content])
    };
    Some((t, j + 1 - i))
}

fn scan(s: &str) -> (Vec<char>, Vec<Found>) {
    let chars: Vec<char> = s.chars().collect();
    let mut found = Vec::new();
    // A pair of hashes highlights what's between them: each hash is a token, the text and codes
    // between them are as anywhere else.
    let mut hash_closes: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        let hash = || Some((token(Family::Code, "#".into(), vec![]), 1));
        let t = match chars[i] {
            '#' if hash_closes == Some(i) => {
                hash_closes = None;
                hash()
            }
            '#' if hash_closes.is_none() => paired(&chars, i, '#').and_then(|j| {
                hash_closes = Some(j);
                hash()
            }),
            '[' => bracket(&chars, i),
            '{' => brace(&chars, i),
            '<' => html(&chars, i),
            '&' => entity(&chars, i),
            '%' => printf(&chars, i),
            '|' => code(&chars, i),
            '\n' => Some((token(Family::LineBreak, "\n".into(), vec![]), 1)),
            _ => None,
        };
        match t {
            Some((token, len)) => {
                found.push(Found {
                    token,
                    start: i,
                    end: i + len,
                });
                i += len;
            }
            None => i += 1,
        }
    }
    (chars, found)
}

/// The markup tokens of `s`, in order.
pub fn tokens(s: &str) -> Vec<Token> {
    scan(s).1.into_iter().map(|f| f.token).collect()
}

/// What `s` draws: its text with its markup taken out, but the markup's visible text kept.
pub fn drawn(s: &str) -> String {
    let (chars, found) = scan(s);
    let mut out = String::new();
    let mut at = 0;
    for f in found {
        out.extend(&chars[at..f.start]);
        for v in &f.token.visible {
            out.push_str(v);
        }
        at = f.end;
    }
    out.extend(&chars[at..]);
    out
}

impl Token {
    /// A tag that can close, `[x …]` or `<x …>`: its name, lowercase, and whether this one closes.
    pub fn tag(&self) -> Option<(String, bool)> {
        let inner = match self.family {
            Family::Bracket => self.shape.strip_prefix('[')?,
            Family::Html => self.shape.strip_prefix('<')?,
            _ => return None,
        };
        let closes = inner.starts_with('/');
        let name: String = inner
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<String>()
            .to_ascii_lowercase();
        (!name.is_empty()).then_some((name, closes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(s: &str) -> Vec<(Family, String, Vec<String>)> {
        tokens(s)
            .into_iter()
            .map(|t| (t.family, t.shape, t.visible))
            .collect()
    }

    fn plain(family: Family, shape: &str) -> (Family, String, Vec<String>) {
        (family, shape.into(), vec![])
    }

    #[test]
    fn bracket_tags_and_the_names_inside_them() {
        assert_eq!(
            shapes("[c 99ccff]剑[/c][size 12][indent][rolename]"),
            [
                plain(Family::Bracket, "[c 99ccff]"),
                plain(Family::Bracket, "[/c]"),
                plain(Family::Bracket, "[size 12]"),
                plain(Family::Bracket, "[indent]"),
                plain(Family::Bracket, "[rolename]"),
            ]
        );
        assert_eq!(
            shapes("[q_npcname text=\"某人\"]说[d_npc name=\"某人\" action=\"\"]"),
            [
                (
                    Family::Bracket,
                    "[q_npcname text=\"\u{1}\"]".into(),
                    vec!["某人".into()]
                ),
                (
                    Family::Bracket,
                    "[d_npc name=\"\u{1}\" action=\"\"]".into(),
                    vec!["某人".into()]
                ),
            ]
        );
        // Brackets around a word, a key's name or a placeholder are punctuation, not a tag:
        // today's English keeps or drops them as it reads best (Task 4's check of it).
        assert!(tokens("拿起[铁剑]按[W]").is_empty());
        assert_eq!(shapes("[{2}]"), [plain(Family::Brace, "{2}")]);
        // Text attributes are the tag's own: a gender's two forms, every class's, but never an
        // id that's called a name.
        assert_eq!(
            shapes("[gender male=\"他\" female='她'][vocation tg=\"甲\" zf=\"乙\"]"),
            [
                (
                    Family::Bracket,
                    "[gender male=\"\u{1}\" female='\u{1}']".into(),
                    vec!["他".into(), "她".into()]
                ),
                (
                    Family::Bracket,
                    "[vocation tg=\"\u{1}\" zf=\"\u{1}\"]".into(),
                    vec!["甲".into(), "乙".into()]
                ),
            ]
        );
        assert_eq!(
            shapes(
                "[functionkey name=\"ui.map\"][q_sound event=\"event:/voice/npc/1\"][money name='Gold']"
            ),
            [
                plain(Family::Bracket, "[functionkey name=\"ui.map\"]"),
                plain(Family::Bracket, "[q_sound event=\"event:/voice/npc/1\"]"),
                plain(Family::Bracket, "[money name='Gold']"),
            ]
        );
        // A translated name keeps its token's shape.
        assert_eq!(
            tokens("[q_npcname text=\"Lin Yue\"]")[0].shape,
            "[q_npcname text=\"\u{1}\"]"
        );
    }

    #[test]
    fn brace_placeholders_some_holding_words() {
        assert_eq!(
            shapes("{0}{0:d}{1:.2f}{target}{color 99ff99 某词}%s"),
            [
                plain(Family::Brace, "{0}"),
                plain(Family::Brace, "{0:d}"),
                plain(Family::Brace, "{1:.2f}"),
                plain(Family::Brace, "{target}"),
                (
                    Family::Brace,
                    "{color 99ff99 \u{1}}".into(),
                    vec!["某词".into()]
                ),
                plain(Family::Brace, "%s"),
            ]
        );
    }

    #[test]
    fn html_tags() {
        assert_eq!(
            shapes("<font color=\"#dfdc9d\">字</font><br><b>&nbsp;"),
            [
                plain(Family::Html, "<font color=\"#dfdc9d\">"),
                plain(Family::Html, "</font>"),
                plain(Family::Html, "<br>"),
                plain(Family::Html, "<b>"),
                plain(Family::Html, "&nbsp;"),
            ]
        );
    }

    #[test]
    fn codes_and_line_breaks() {
        // A pair of hashes highlights the text between them, which is text and codes as anywhere.
        assert_eq!(
            shapes("造成|cp,1,~|点#某词#\n#|cp,2|秒#"),
            [
                plain(Family::Code, "|cp,1,~|"),
                plain(Family::Code, "#"),
                plain(Family::Code, "#"),
                plain(Family::LineBreak, "\n"),
                plain(Family::Code, "#"),
                plain(Family::Code, "|cp,2|"),
                plain(Family::Code, "#"),
            ]
        );
        assert_eq!(drawn("点#某词#"), "点某词");
    }

    #[test]
    fn link_texts_and_buttons_are_slots() {
        assert_eq!(
            shapes("[link type='bookjour' text='第{1}页' Position='{3}']<w button=\"确定\" />"),
            [
                (
                    Family::Bracket,
                    "[link type='bookjour' text='\u{1}' Position='{3}']".into(),
                    vec!["第{1}页".into()]
                ),
                (
                    Family::Html,
                    "<w button=\"\u{1}\" />".into(),
                    vec!["确定".into()]
                ),
            ]
        );
    }

    #[test]
    fn text_is_not_a_token() {
        for text in [
            "一把剑。",
            "100%",
            "a < b",
            "[",
            "a | b",
            "#1 in line",
            "{ }",
        ] {
            assert!(tokens(text).is_empty(), "{text:?}: {:?}", tokens(text));
        }
    }
}
