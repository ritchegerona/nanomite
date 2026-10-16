//! The slash-command registry.
//!
//! Mirrors opencode's built-in command set so muscle memory carries over: every command
//! opencode ships has an entry here, and `hint` is what the inline `/` list shows.

/// A command that can be typed in the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Connect,
    Compact,
    Details,
    Editor,
    Exit,
    Export,
    Help,
    Init,
    Models,
    New,
    Reasoning,
    Redo,
    Sessions,
    Share,
    Themes,
    Thinking,
    Undo,
    Unshare,
    Memory,
    Copy,
    ProviderMenu,
    Tools,
    Panel,
    Improve,
    ImproveFromBoard,
    Prev,
    Next,
}

pub struct Entry {
    /// Canonical name, e.g. `/compact`.
    pub command: &'static str,
    /// Extra names opencode accepts for the same command.
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
    pub kind: Command,
}

/// Ordered like opencode's documentation.
pub const COMMANDS: &[Entry] = &[
    Entry {
        command: "/connect",
        aliases: &["/login", "/auth"],
        hint: "edit the endpoint url and api key",
        kind: Command::Connect,
    },
    Entry {
        command: "/provider",
        aliases: &["/providers"],
        hint: "switch provider - pick from the list",
        kind: Command::ProviderMenu,
    },
    Entry {
        command: "/compact",
        aliases: &["/summarize"],
        hint: "summarise the session to free up context",
        kind: Command::Compact,
    },
    Entry {
        command: "/details",
        aliases: &[],
        hint: "toggle tool output details",
        kind: Command::Details,
    },
    Entry {
        command: "/editor",
        aliases: &[],
        hint: "compose the prompt in $EDITOR",
        kind: Command::Editor,
    },
    Entry {
        command: "/exit",
        aliases: &["/quit", "/q"],
        hint: "quit nanomite",
        kind: Command::Exit,
    },
    Entry {
        command: "/export",
        aliases: &[],
        hint: "write the session to markdown",
        kind: Command::Export,
    },
    Entry {
        command: "/help",
        aliases: &[],
        hint: "keyboard shortcuts and commands",
        kind: Command::Help,
    },
    Entry {
        command: "/init",
        aliases: &[],
        hint: "create or update AGENTS.md",
        kind: Command::Init,
    },
    Entry {
        command: "/models",
        aliases: &["/model"],
        hint: "switch model - filter or type an id",
        kind: Command::Models,
    },
    Entry {
        command: "/new",
        aliases: &["/clear"],
        hint: "start a new session",
        kind: Command::New,
    },
    Entry {
        command: "/reasoning",
        aliases: &["/effort"],
        hint: "cycle minimal · low · medium · high",
        kind: Command::Reasoning,
    },
    Entry {
        command: "/redo",
        aliases: &[],
        hint: "restore the last undone exchange",
        kind: Command::Redo,
    },
    Entry {
        command: "/sessions",
        aliases: &["/resume", "/continue"],
        hint: "list and switch sessions",
        kind: Command::Sessions,
    },
    Entry {
        command: "/share",
        aliases: &[],
        hint: "export a shareable copy of the session",
        kind: Command::Share,
    },
    Entry {
        command: "/memory",
        aliases: &["/mem"],
        hint: "browse stored memories",
        kind: Command::Memory,
    },
    Entry {
        command: "/themes",
        aliases: &["/theme"],
        hint: "switch colour theme",
        kind: Command::Themes,
    },
    Entry {
        command: "/thinking",
        aliases: &[],
        hint: "show or hide the thinking indicator",
        kind: Command::Thinking,
    },
    Entry {
        command: "/undo",
        aliases: &[],
        hint: "drop the last exchange",
        kind: Command::Undo,
    },
    Entry {
        command: "/unshare",
        aliases: &[],
        hint: "remove any shared copy",
        kind: Command::Unshare,
    },
    Entry {
        command: "/prev",
        aliases: &[],
        hint: "browse to previous conversation turn",
        kind: Command::Prev,
    },
    Entry {
        command: "/next",
        aliases: &[],
        hint: "browse to next conversation turn",
        kind: Command::Next,
    },
    Entry {
        command: "/copy",
        aliases: &[],
        hint: "copy selection to clipboard",
        kind: Command::Copy,
    },
    Entry {
        command: "/tools",
        aliases: &[],
        hint: "tool library - enable or disable agent tools",
        kind: Command::Tools,
    },
    Entry {
        command: "/panel",
        aliases: &[],
        hint: "convene the agent board - /panel <topic>",
        kind: Command::Panel,
    },
    Entry {
        command: "/improve",
        aliases: &[],
        hint: "review self-edit proposals",
        kind: Command::Improve,
    },
    Entry {
        command: "/improve-from-board",
        aliases: &[],
        hint: "stage the board's chair synthesis as an improve proposal",
        kind: Command::ImproveFromBoard,
    },
];

pub fn lookup(word: &str) -> Option<&'static Entry> {
    let word = word.trim().to_ascii_lowercase();
    COMMANDS.iter().find(|c| {
        c.command.trim_start_matches('/') == word
            || c.aliases.iter().any(|a| a.trim_start_matches('/') == word)
    })
}

/// The command behind a composer string, if it is one.
pub fn parse(text: &str) -> Option<(Command, &'static str)> {
    let body = text.trim().strip_prefix('/')?;
    let name = body.split_whitespace().next().unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    lookup(name).map(|entry| (entry.kind, entry.command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_parses_under_its_name_and_aliases() {
        for entry in COMMANDS {
            let (kind, canonical) = parse(entry.command).expect(entry.command);
            assert_eq!(kind, entry.kind);
            assert_eq!(canonical, entry.command);
            for alias in entry.aliases {
                let (kind, _) =
                    parse(alias).unwrap_or_else(|| panic!("alias {alias} does not parse"));
                assert_eq!(kind, entry.kind, "alias {alias} maps elsewhere");
            }
        }
    }

    #[test]
    fn names_and_aliases_are_unique() {
        let mut names: Vec<String> = COMMANDS
            .iter()
            .flat_map(|c| {
                std::iter::once(c.command.to_string())
                    .chain(c.aliases.iter().map(|a| a.to_string()))
            })
            .collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate command names");
    }

    #[test]
    fn opencode_command_set_is_covered() {
        for name in [
            "/connect",
            "/compact",
            "/details",
            "/editor",
            "/exit",
            "/export",
            "/help",
            "/improve",
            "/init",
            "/memory",
            "/models",
            "/new",
            "/redo",
            "/sessions",
            "/share",
            "/themes",
            "/thinking",
            "/undo",
            "/unshare",
        ] {
            assert!(
                lookup(name.trim_start_matches('/')).is_some(),
                "{name} missing"
            );
        }
    }

    #[test]
    fn non_commands_are_not_intercepted() {
        assert!(parse("/nope").is_none());
        assert!(parse("hello /help").is_none());
        assert!(parse("/").is_none());
    }
}
