//! Launch options (launcher spec Amendment B, LB3; L25): Steam's form,
//! `NAME=value … wrapper … %command% arguments…`, split into words as a POSIX shell splits them,
//! with its quotes and backslashes, and never run through one: variables, globs, `~` and a
//! shell's operators stay as they're written.

/// What launch options ask for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// The leading `NAME=value` words: the start's environment, making the prefix's too.
    pub env: Vec<(String, String)>,
    /// The words before `%command%`: what the game starts under, outermost first.
    pub wrappers: Vec<String>,
    /// The words after `%command%`, or all of them without it, as in Steam: the game's added
    /// arguments, after Fenji's own.
    pub args: Vec<String>,
}

/// Why launch options are refused; the model says it in words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OptionsError {
    #[error("a quote isn't closed")]
    Unclosed,
    #[error("%command% is there twice")]
    CommandTwice,
    /// A variable Fenji sets for the start, by the name the options gave it.
    #[error("{0} is Fenji's own")]
    Reserved(String),
}

/// Where Steam's form puts the start.
const COMMAND: &str = "%command%";

/// A word as split: each character, and whether a quote or a backslash made it literal.
type Word = Vec<(char, bool)>;

/// `text`'s words, split as a POSIX shell splits them.
fn split(text: &str) -> Result<Vec<Word>, OptionsError> {
    let mut words = Vec::new();
    let mut word: Option<Word> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => words.extend(word.take()),
            '\\' => match chars.next() {
                // A backslash before a newline joins the lines.
                Some('\n') => {}
                Some(next) => word.get_or_insert_default().push((next, true)),
                // A lone one at the end stays, as `sh -c` keeps it.
                None => word.get_or_insert_default().push(('\\', false)),
            },
            '\'' => {
                let w = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => w.push((c, true)),
                        None => return Err(OptionsError::Unclosed),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        // Within double quotes, a backslash means something only before these.
                        Some('\\') => match chars.peek() {
                            Some('\n') => {
                                chars.next();
                            }
                            Some(&next @ ('$' | '`' | '"' | '\\')) => {
                                chars.next();
                                w.push((next, true));
                            }
                            _ => w.push(('\\', true)),
                        },
                        Some(c) => w.push((c, true)),
                        None => return Err(OptionsError::Unclosed),
                    }
                }
            }
            c => word.get_or_insert_default().push((c, false)),
        }
    }
    words.extend(word);
    Ok(words)
}

fn joined(chars: &[(char, bool)]) -> String {
    chars.iter().map(|&(c, _)| c).collect()
}

/// A word's `NAME=value`, when it starts with a name and an `=` that no quote or backslash
/// touched, as a shell's assignment does.
fn assignment(word: &Word) -> Option<(String, String)> {
    let eq = word.iter().position(|&(c, _)| c == '=')?;
    let (name, rest) = word.split_at(eq);
    if rest[0].1 || name.iter().any(|&(_, literal)| literal) {
        return None;
    }
    let name = joined(name);
    let mut chars = name.chars();
    let first = chars.next()?;
    let named = (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric());
    named.then(|| (name.clone(), joined(&rest[1..])))
}

/// `%command%` itself, with no quote or backslash.
fn is_command(word: &Word) -> bool {
    word.iter().all(|&(_, literal)| !literal) && joined(word) == COMMAND
}

/// The variables Fenji sets for the start: umu-run's prefix and game id, and the login path's
/// (`gj_*`), compared without regard to case, as Wine looks variables up.
fn reserved(name: &str) -> bool {
    name.eq_ignore_ascii_case("WINEPREFIX")
        || name.eq_ignore_ascii_case("GAMEID")
        || name.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("gj_"))
}

/// Launch options from the player's text.
pub fn parse(text: &str) -> Result<LaunchOptions, OptionsError> {
    let words = split(text)?;
    let mut env = Vec::new();
    let mut rest = words.as_slice();
    while let Some((first, more)) = rest.split_first() {
        let Some((name, value)) = assignment(first) else {
            break;
        };
        if reserved(&name) {
            return Err(OptionsError::Reserved(name));
        }
        env.push((name, value));
        rest = more;
    }
    let marks: Vec<usize> = (0..rest.len()).filter(|&i| is_command(&rest[i])).collect();
    let (wrappers, args) = match marks.as_slice() {
        [] => (&rest[..0], rest),
        &[at] => (&rest[..at], &rest[at + 1..]),
        _ => return Err(OptionsError::CommandTwice),
    };
    let words = |list: &[Word]| list.iter().map(|w| joined(w)).collect();
    Ok(LaunchOptions {
        env,
        wrappers: words(wrappers),
        args: words(args),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_string()).collect()
    }

    fn env(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn nothing_asks_for_nothing() {
        for text in ["", "   ", "\t\n "] {
            assert_eq!(parse(text), Ok(LaunchOptions::default()), "{text:?}");
        }
    }

    #[test]
    fn steams_form_splits_into_variables_wrappers_and_arguments() {
        let o =
            parse("PROTON_LOG=1 DXVK_HUD=fps mangohud gamescope -f -- %command% -dx11 -nosplash")
                .unwrap();
        assert_eq!(o.env, env(&[("PROTON_LOG", "1"), ("DXVK_HUD", "fps")]));
        assert_eq!(o.wrappers, words(&["mangohud", "gamescope", "-f", "--"]));
        assert_eq!(o.args, words(&["-dx11", "-nosplash"]));
        // Without %command%, as in Steam, the words are the game's arguments; leading variables
        // are still variables.
        let o = parse("PROTON_LOG=1 -dx11").unwrap();
        assert_eq!(o.env, env(&[("PROTON_LOG", "1")]));
        assert!(o.wrappers.is_empty());
        assert_eq!(o.args, words(&["-dx11"]));
        // %command% alone, first or last.
        assert_eq!(parse("%command%"), Ok(LaunchOptions::default()));
        let o = parse("%command% -windowed").unwrap();
        assert_eq!((o.wrappers.len(), o.args), (0, words(&["-windowed"])));
        let o = parse("gamemoderun %command%").unwrap();
        assert_eq!((o.wrappers, o.args.len()), (words(&["gamemoderun"]), 0));
    }

    #[test]
    fn quotes_and_backslashes_work_as_a_shells_do() {
        let o = parse(r#"A="two words" B='it'"'"'s' C=a\ b %command% "-name=Lin Yue" 'x y' a\"b "q\"uote" "back\\slash" "keep\d" ''"#)
            .unwrap();
        assert_eq!(
            o.env,
            env(&[("A", "two words"), ("B", "it's"), ("C", "a b")])
        );
        assert_eq!(
            o.args,
            words(&[
                "-name=Lin Yue",
                "x y",
                "a\"b",
                "q\"uote",
                "back\\slash",
                "keep\\d",
                ""
            ])
        );
        // Single quotes keep backslashes as they are; a lone backslash at the end is kept, as
        // `sh -c` keeps it; a backslash before a newline joins the lines.
        let o = parse(r"'C:\Games' end\").unwrap();
        assert_eq!(o.args, words(&["C:\\Games", "end\\"]));
        let o = parse("one\\\ntwo").unwrap();
        assert_eq!(o.args, words(&["onetwo"]));
        // No variables, globs or home: never a shell.
        let o = parse("$HOME ~/x *.log `id` $(id) ; && |").unwrap();
        assert_eq!(
            o.args,
            words(&["$HOME", "~/x", "*.log", "`id`", "$(id)", ";", "&&", "|"])
        );
    }

    #[test]
    fn only_a_plain_leading_name_equals_value_is_a_variable() {
        // After the first other word, NAME=value is an argument, as to `env` here.
        let o = parse("env FOO=bar %command%").unwrap();
        assert!(o.env.is_empty());
        assert_eq!(o.wrappers, words(&["env", "FOO=bar"]));
        // A quoted or escaped name, or one that isn't a name, makes a word, as in a shell.
        for text in [
            r#""FOO=bar""#,
            r"FOO\=bar",
            "'FOO'=bar",
            "1FOO=bar",
            "=bar",
            "FO-O=bar",
        ] {
            let o = parse(text).unwrap();
            assert!(o.env.is_empty(), "{text}");
            assert_eq!(o.args.len(), 1, "{text}");
        }
        // The value may be quoted, empty, or hold more =.
        let o = parse(r#"A= B="" C=x=y D='%command%' %command%"#).unwrap();
        assert_eq!(
            o.env,
            env(&[("A", ""), ("B", ""), ("C", "x=y"), ("D", "%command%")])
        );
    }

    #[test]
    fn a_quoted_or_escaped_command_is_a_word_like_any_other() {
        for text in [
            r"'%command%' %command% x",
            r#""%command%" %command% x"#,
            r"\%command% %command% x",
        ] {
            let o = parse(text).unwrap();
            assert_eq!(o.wrappers, words(&["%command%"]), "{text}");
            assert_eq!(o.args, words(&["x"]), "{text}");
        }
        // Joined to other text, it isn't %command% either.
        let o = parse("%command%x").unwrap();
        assert_eq!(o.args, words(&["%command%x"]));
    }

    #[test]
    fn each_refusal_says_what_it_refuses() {
        for text in ["'open", "\"open", "a 'b c", "FOO=\"bar", "a \"b\\\""] {
            assert_eq!(parse(text), Err(OptionsError::Unclosed), "{text}");
        }
        assert_eq!(
            parse("mangohud %command% -x %command%"),
            Err(OptionsError::CommandTwice)
        );
        for (text, name) in [
            ("WINEPREFIX=/tmp/p %command%", "WINEPREFIX"),
            ("A=1 GAMEID=umu-0 %command%", "GAMEID"),
            ("gj_ticket=x", "gj_ticket"),
            ("GJ_GATES=10.0.0.9:7800", "GJ_GATES"),
            ("wineprefix=/tmp/p", "wineprefix"),
        ] {
            assert_eq!(
                parse(text),
                Err(OptionsError::Reserved(name.into())),
                "{text}"
            );
        }
        // A player's own Proton is theirs to name.
        let o = parse("PROTONPATH=/home/me/proton %command%").unwrap();
        assert_eq!(o.env, env(&[("PROTONPATH", "/home/me/proton")]));
    }
}
