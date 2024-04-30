use std::collections::{HashMap, HashSet};

use crossterm::style::{Color, ResetColor, SetAttribute, SetForegroundColor};

use atuin_client::{history::History, settings::Settings};
use unicode_segmentation::UnicodeSegmentation;

pub struct Stats<'a> {
    pub total_commands: usize,
    pub unique_commands: usize,
    pub top: Vec<(Vec<&'a str>, usize)>,
}

fn first_non_whitespace(s: &str) -> Option<usize> {
    s.char_indices()
        // find the first non whitespace char
        .find(|(_, c)| !c.is_ascii_whitespace())
        // return the index of that char
        .map(|(i, _)| i)
}

fn first_whitespace(s: &str) -> usize {
    s.char_indices()
        // find the first whitespace char
        .find(|(_, c)| c.is_ascii_whitespace())
        // return the index of that char, (or the max length of the string)
        .map_or(s.len(), |(i, _)| i)
}

fn interesting_command<'a>(settings: &Settings, mut command: &'a str) -> &'a str {
    // Sort by length so that we match the longest prefix first
    let mut common_prefix = settings.stats.common_prefix.clone();
    common_prefix.sort_by_key(|b| std::cmp::Reverse(b.len()));

    // Trim off the common prefix, if it exists
    for p in &common_prefix {
        if command.starts_with(p) {
            let i = p.len();
            let prefix = &command[..i];
            command = command[i..].trim_start();
            if command.is_empty() {
                // no commands following, just use the prefix
                return prefix;
            }
            break;
        }
    }

    // Sort the common_subcommands by length so that we match the longest subcommand first
    let mut common_subcommands = settings.stats.common_subcommands.clone();
    common_subcommands.sort_by_key(|b| std::cmp::Reverse(b.len()));

    // Check for a common subcommand
    for p in &common_subcommands {
        if command.starts_with(p) {
            // if the subcommand is the same length as the command, then we just use the subcommand
            if p.len() == command.len() {
                return command;
            }
            // otherwise we need to use the subcommand + the next word
            let non_whitespace = first_non_whitespace(&command[p.len()..]).unwrap_or(0);
            let j =
                p.len() + non_whitespace + first_whitespace(&command[p.len() + non_whitespace..]);
            return &command[..j];
        }
    }
    // Return the first word if there is no subcommand
    &command[..first_whitespace(command)]
}

fn split_at_pipe(command: &str) -> Vec<&str> {
    let mut result = vec![];
    let mut quoted = false;
    let mut start = 0;
    let mut graphemes = UnicodeSegmentation::grapheme_indices(command, true);

    while let Some((i, c)) = graphemes.next() {
        let current = i;
        match c {
            "\"" => {
                if command[start..current] != *"\"" {
                    quoted = !quoted;
                }
            }
            "'" => {
                if command[start..current] != *"'" {
                    quoted = !quoted;
                }
            }
            "\\" => if graphemes.next().is_some() {},
            "|" => {
                if !quoted {
                    if command[start..].starts_with('|') {
                        start += 1;
                    }
                    result.push(&command[start..current]);
                    start = current;
                }
            }
            _ => {}
        }
    }
    if command[start..].starts_with('|') {
        start += 1;
    }
    result.push(&command[start..]);
    result
}

pub fn pretty_print(stats: Stats, ngram_size: usize) {
    let max = stats.top.iter().map(|x| x.1).max().unwrap();
    let num_pad = max.ilog10() as usize + 1;

    // Find the length of the longest command name for each column
    let column_widths = stats
        .top
        .iter()
        .map(|(commands, _)| commands.iter().map(|c| c.len()).collect::<Vec<usize>>())
        .fold(vec![0; ngram_size], |acc, item| {
            acc.iter()
                .zip(item.iter())
                .map(|(a, i)| *std::cmp::max(a, i))
                .collect()
        });

    for (command, count) in stats.top {
        let gray = SetForegroundColor(Color::Grey);
        let bold = SetAttribute(crossterm::style::Attribute::Bold);

        let in_ten = 10 * count / max;

        print!("[");
        print!("{}", SetForegroundColor(Color::Red));

        for i in 0..in_ten {
            if i == 2 {
                print!("{}", SetForegroundColor(Color::Yellow));
            }

            if i == 5 {
                print!("{}", SetForegroundColor(Color::Green));
            }

            print!("▮");
        }

        for _ in in_ten..10 {
            print!(" ");
        }

        let formatted_command = command
            .iter()
            .zip(column_widths.iter())
            .map(|(cmd, width)| format!("{cmd:width$}"))
            .collect::<Vec<_>>()
            .join(" | ");

        println!("{ResetColor}] {gray}{count:num_pad$}{ResetColor} {bold}{formatted_command}{ResetColor}");
    }
    println!("Total commands:   {}", stats.total_commands);
    println!("Unique commands:  {}", stats.unique_commands);
}

pub fn compute<'a>(
    settings: &Settings,
    history: &'a [History],
    count: usize,
    ngram_size: usize,
) -> Option<Stats<'a>> {
    let mut commands = HashSet::<&str>::with_capacity(history.len());
    let mut total_unignored = 0;
    let mut prefixes = HashMap::<Vec<&str>, usize>::with_capacity(history.len());

    for i in history {
        // just in case it somehow has a leading tab or space or something (legacy atuin didn't ignore space prefixes)
        let command = i.command.trim();
        let prefix = interesting_command(settings, command);

        if settings.stats.ignored_commands.iter().any(|c| c == prefix) {
            continue;
        }

        total_unignored += 1;
        commands.insert(command);

        split_at_pipe(i.command.trim())
            .iter()
            .map(|l| {
                let command = l.trim();
                commands.insert(command);
                command
            })
            .collect::<Vec<_>>()
            .windows(ngram_size)
            .for_each(|w| {
                *prefixes
                    .entry(w.iter().map(|c| interesting_command(settings, c)).collect())
                    .or_default() += 1;
            });
    }

    let unique = commands.len();
    let mut top = prefixes.into_iter().collect::<Vec<_>>();

    top.sort_unstable_by_key(|x| std::cmp::Reverse(x.1));
    top.truncate(count);

    if top.is_empty() {
        return None;
    }

    Some(Stats {
        unique_commands: unique,
        total_commands: total_unignored,
        top,
    })
}

#[cfg(test)]
mod tests {
    use atuin_client::history::History;
    use atuin_client::settings::Settings;
    use time::OffsetDateTime;

    use super::compute;
    use super::{interesting_command, split_at_pipe};

    #[test]
    fn ignored_commands() {
        let mut settings = Settings::utc();
        settings.stats.ignored_commands.push("cd".to_string());

        let history = [
            History::import()
                .timestamp(OffsetDateTime::now_utc())
                .command("cd foo")
                .build()
                .into(),
            History::import()
                .timestamp(OffsetDateTime::now_utc())
                .command("cargo build stuff")
                .build()
                .into(),
        ];

        let stats = compute(&settings, &history, 10, 1).expect("failed to compute stats");
        assert_eq!(stats.total_commands, 1);
        assert_eq!(stats.unique_commands, 1);
    }

    #[test]
    fn interesting_commands() {
        let settings = Settings::utc();

        assert_eq!(interesting_command(&settings, "cargo"), "cargo");
        assert_eq!(
            interesting_command(&settings, "cargo build foo bar"),
            "cargo build"
        );
        assert_eq!(
            interesting_command(&settings, "sudo   cargo build foo bar"),
            "cargo build"
        );
        assert_eq!(interesting_command(&settings, "sudo"), "sudo");
    }

    // Test with spaces in the common_prefix
    #[test]
    fn interesting_commands_spaces() {
        let mut settings = Settings::utc();
        settings.stats.common_prefix.push("sudo test".to_string());

        assert_eq!(interesting_command(&settings, "sudo test"), "sudo test");
        assert_eq!(interesting_command(&settings, "sudo test  "), "sudo test");
        assert_eq!(interesting_command(&settings, "sudo test foo bar"), "foo");
        assert_eq!(
            interesting_command(&settings, "sudo test    foo bar"),
            "foo"
        );

        // Works with a common_subcommand as well
        assert_eq!(
            interesting_command(&settings, "sudo test cargo build foo bar"),
            "cargo build"
        );

        // We still match on just the sudo prefix
        assert_eq!(interesting_command(&settings, "sudo"), "sudo");
        assert_eq!(interesting_command(&settings, "sudo foo"), "foo");
    }

    // Test with spaces in the common_subcommand
    #[test]
    fn interesting_commands_spaces_subcommand() {
        let mut settings = Settings::utc();
        settings
            .stats
            .common_subcommands
            .push("cargo build".to_string());

        assert_eq!(interesting_command(&settings, "cargo build"), "cargo build");
        assert_eq!(
            interesting_command(&settings, "cargo build   "),
            "cargo build"
        );
        assert_eq!(
            interesting_command(&settings, "cargo build foo bar"),
            "cargo build foo"
        );

        // Works with a common_prefix as well
        assert_eq!(
            interesting_command(&settings, "sudo cargo build foo bar"),
            "cargo build foo"
        );

        // We still match on just cargo as a subcommand
        assert_eq!(interesting_command(&settings, "cargo"), "cargo");
        assert_eq!(interesting_command(&settings, "cargo foo"), "cargo foo");
    }

    // Test with spaces in the common_prefix and common_subcommand
    #[test]
    fn interesting_commands_spaces_both() {
        let mut settings = Settings::utc();
        settings.stats.common_prefix.push("sudo test".to_string());
        settings
            .stats
            .common_subcommands
            .push("cargo build".to_string());

        assert_eq!(
            interesting_command(&settings, "sudo test cargo build"),
            "cargo build"
        );
        assert_eq!(
            interesting_command(&settings, "sudo test   cargo build"),
            "cargo build"
        );
        assert_eq!(
            interesting_command(&settings, "sudo test cargo build   "),
            "cargo build"
        );
        assert_eq!(
            interesting_command(&settings, "sudo test cargo build foo bar"),
            "cargo build foo"
        );
    }

    #[test]
    fn split_simple() {
        assert_eq!(split_at_pipe("fd | rg"), ["fd ", " rg"]);
    }

    #[test]
    fn split_multi() {
        assert_eq!(
            split_at_pipe("kubectl | jq | rg"),
            ["kubectl ", " jq ", " rg"]
        );
    }

    #[test]
    fn split_simple_quoted() {
        assert_eq!(
            split_at_pipe("foo | bar 'baz {} | quux' | xyzzy"),
            ["foo ", " bar 'baz {} | quux' ", " xyzzy"]
        );
    }

    #[test]
    fn split_multi_quoted() {
        assert_eq!(
            split_at_pipe("foo | bar 'baz \"{}\" | quux' | xyzzy"),
            ["foo ", " bar 'baz \"{}\" | quux' ", " xyzzy"]
        );
    }

    #[test]
    fn escaped_pipes() {
        assert_eq!(
            split_at_pipe("foo | bar baz \\| quux"),
            ["foo ", " bar baz \\| quux"]
        );
    }

    #[test]
    fn emoji() {
        assert_eq!(
            split_at_pipe("git commit -m \"🚀\""),
            ["git commit -m \"🚀\""]
        );
    }
}
