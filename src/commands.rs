//! The slash-command registry.
//!
//! Mirrors opencode's built-in command set so muscle memory carries over.
//! Entries are grouped by purpose — connect & model, agent scope, session,
//! transcript, agent features, appearance, output — and the palette shows
//! them in that order; `hint` is the one-line description the `/` list shows.

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
    Policy,
    Worktrees,
    Hud,
    Revert,
    Fork,
    History,
    Doctor,
    Ab,
    Focus,
    Team,
}

pub struct Entry {
    /// Canonical name, e.g. `/compact`.
    pub command: &'static str,
    /// Extra names opencode accepts for the same command.
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
    pub kind: Command,
}

/// Grouped by purpose, in the order the palette lists them.
pub const COMMANDS: &[Entry] = &[
    // -- connect & model ---------------------------------------------------
    Entry {
        command: "/connect",
        aliases: &["/login", "/auth"],
        hint: "edit the endpoint url and api key",
        kind: Command::Connect,
    },
    Entry {
        command: "/provider",
        aliases: &["/providers"],
        hint: "switch the provider - pick from the list",
        kind: Command::ProviderMenu,
    },
    Entry {
        command: "/models",
        aliases: &["/model"],
        hint: "switch the model - filter or type an id",
        kind: Command::Models,
    },
    Entry {
        command: "/reasoning",
        aliases: &["/effort"],
        hint: "cycle reasoning effort: minimal · low · medium · high",
        kind: Command::Reasoning,
    },
    // -- agent scope ---------------------------------------------------------
    Entry {
        command: "/policy",
        aliases: &["/mode"],
        hint: "cycle tool mode: plan · coder · build · hacker",
        kind: Command::Policy,
    },
    Entry {
        command: "/tools",
        aliases: &[],
        hint: "tool library - enable or disable agent tools",
        kind: Command::Tools,
    },
    Entry {
        command: "/worktrees",
        aliases: &["/wt"],
        hint: "list git worktrees for parallel agents",
        kind: Command::Worktrees,
    },
    // -- session -------------------------------------------------------------
    Entry {
        command: "/new",
        aliases: &["/clear"],
        hint: "start a new session",
        kind: Command::New,
    },
    Entry {
        command: "/sessions",
        aliases: &["/resume", "/continue"],
        hint: "list and switch sessions",
        kind: Command::Sessions,
    },
    Entry {
        command: "/history",
        aliases: &[],
        hint: "search all saved sessions - /history <query>",
        kind: Command::History,
    },
    Entry {
        command: "/fork",
        aliases: &[],
        hint: "branch the session at turn N - /fork <n>",
        kind: Command::Fork,
    },
    Entry {
        command: "/compact",
        aliases: &["/summarize"],
        hint: "summarise the session to free up context",
        kind: Command::Compact,
    },
    Entry {
        command: "/undo",
        aliases: &[],
        hint: "drop the last exchange",
        kind: Command::Undo,
    },
    Entry {
        command: "/redo",
        aliases: &[],
        hint: "restore the last undone exchange",
        kind: Command::Redo,
    },
    Entry {
        command: "/revert",
        aliases: &[],
        hint: "time machine - restore files to their state before a turn",
        kind: Command::Revert,
    },
    // -- transcript ----------------------------------------------------------
    Entry {
        command: "/prev",
        aliases: &[],
        hint: "browse to the previous turn",
        kind: Command::Prev,
    },
    Entry {
        command: "/next",
        aliases: &[],
        hint: "browse to the next turn",
        kind: Command::Next,
    },
    Entry {
        command: "/copy",
        aliases: &[],
        hint: "copy the last assistant message",
        kind: Command::Copy,
    },
    Entry {
        command: "/editor",
        aliases: &[],
        hint: "compose the prompt in $EDITOR",
        kind: Command::Editor,
    },
    // -- agent features --------------------------------------------------------
    Entry {
        command: "/memory",
        aliases: &["/mem"],
        hint: "browse stored memories",
        kind: Command::Memory,
    },
    Entry {
        command: "/panel",
        aliases: &[],
        hint: "convene the agent board - /panel <topic>",
        kind: Command::Panel,
    },
    Entry {
        command: "/ab",
        aliases: &[],
        hint: "compare minimal vs high reasoning side by side - /ab <prompt>",
        kind: Command::Ab,
    },
    Entry {
        command: "/team",
        aliases: &["/parallel"],
        hint: "run three agents in parallel on one goal - /team <goal>",
        kind: Command::Team,
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
        hint: "stage the board's synthesis as a proposal",
        kind: Command::ImproveFromBoard,
    },
    // -- appearance ------------------------------------------------------------
    Entry {
        command: "/details",
        aliases: &[],
        hint: "cycle tool display: hidden · rows · full (ctrl+o)",
        kind: Command::Details,
    },
    Entry {
        command: "/thinking",
        aliases: &[],
        hint: "show or hide the thinking indicator",
        kind: Command::Thinking,
    },
    Entry {
        command: "/focus",
        aliases: &[],
        hint: "show only the latest exchange (f2)",
        kind: Command::Focus,
    },
    Entry {
        command: "/themes",
        aliases: &["/theme"],
        hint: "switch colour theme",
        kind: Command::Themes,
    },
    Entry {
        command: "/hud",
        aliases: &[],
        hint: "toggle the performance hud (frame time, transcript stats)",
        kind: Command::Hud,
    },
    // -- output & general --------------------------------------------------------
    Entry {
        command: "/export",
        aliases: &[],
        hint: "write the session to markdown",
        kind: Command::Export,
    },
    Entry {
        command: "/share",
        aliases: &[],
        hint: "export a shareable copy",
        kind: Command::Share,
    },
    Entry {
        command: "/unshare",
        aliases: &[],
        hint: "remove any shared copy",
        kind: Command::Unshare,
    },
    Entry {
        command: "/init",
        aliases: &[],
        hint: "create or update AGENTS.md",
        kind: Command::Init,
    },
    Entry {
        command: "/doctor",
        aliases: &[],
        hint: "run pre-flight diagnostics on the setup",
        kind: Command::Doctor,
    },
    Entry {
        command: "/help",
        aliases: &[],
        hint: "keyboard shortcuts and commands",
        kind: Command::Help,
    },
    Entry {
        command: "/exit",
        aliases: &["/quit", "/q"],
        hint: "quit nanomite",
        kind: Command::Exit,
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
