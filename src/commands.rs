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
    Image,
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
    EditTurn,
    History,
    Doctor,
    Ab,
    Focus,
    Team,
    Density,
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
        hint: "edit the endpoint url and api key (alt+c)",
        kind: Command::Connect,
    },
    Entry {
        command: "/provider",
        aliases: &["/providers"],
        hint: "switch the provider - pick from the list (alt+p)",
        kind: Command::ProviderMenu,
    },
    Entry {
        command: "/models",
        aliases: &["/model"],
        hint: "switch the model - filter or type an id (alt+m)",
        kind: Command::Models,
    },
    Entry {
        command: "/reasoning",
        aliases: &["/effort"],
        hint: "cycle reasoning effort: minimal · low · medium · high (ctrl+r)",
        kind: Command::Reasoning,
    },
    // -- agent scope ---------------------------------------------------------
    Entry {
        command: "/policy",
        aliases: &["/mode"],
        hint: "cycle tool mode: plan · coder · build · hacker (shift+tab)",
        kind: Command::Policy,
    },
    Entry {
        command: "/tools",
        aliases: &[],
        hint: "tool library - enable or disable agent tools (alt+l)",
        kind: Command::Tools,
    },
    Entry {
        command: "/worktrees",
        aliases: &["/wt"],
        hint: "list git worktrees for parallel agents (alt+w)",
        kind: Command::Worktrees,
    },
    // -- session -------------------------------------------------------------
    Entry {
        command: "/new",
        aliases: &["/clear"],
        hint: "start a new session (ctrl+t)",
        kind: Command::New,
    },
    Entry {
        command: "/sessions",
        aliases: &["/resume", "/continue"],
        hint: "list and switch sessions (alt+s)",
        kind: Command::Sessions,
    },
    Entry {
        command: "/history",
        aliases: &[],
        hint: "search all saved sessions - /history <query> (alt+h)",
        kind: Command::History,
    },
    Entry {
        command: "/fork",
        aliases: &[],
        hint: "branch the session at turn N - /fork <n> (alt+f)",
        kind: Command::Fork,
    },
    Entry {
        command: "/edit",
        aliases: &[],
        hint: "rewind to turn <n>, load its prompt for editing and resend (alt+w)",
        kind: Command::EditTurn,
    },
    Entry {
        command: "/compact",
        aliases: &["/summarize"],
        hint: "summarise the session to free up context (alt+g)",
        kind: Command::Compact,
    },
    Entry {
        command: "/undo",
        aliases: &[],
        hint: "drop the last exchange (alt+u)",
        kind: Command::Undo,
    },
    Entry {
        command: "/redo",
        aliases: &[],
        hint: "restore the last undone exchange (alt+shift+u)",
        kind: Command::Redo,
    },
    Entry {
        command: "/revert",
        aliases: &[],
        hint: "time machine - restore files to their state before a turn (alt+r)",
        kind: Command::Revert,
    },
    // -- transcript ----------------------------------------------------------
    Entry {
        command: "/prev",
        aliases: &[],
        hint: "browse to the previous turn (alt+,)",
        kind: Command::Prev,
    },
    Entry {
        command: "/next",
        aliases: &[],
        hint: "browse to the next turn (alt+.)",
        kind: Command::Next,
    },
    Entry {
        command: "/copy",
        aliases: &[],
        hint: "copy the last assistant message (alt+y)",
        kind: Command::Copy,
    },
    Entry {
        command: "/editor",
        aliases: &[],
        hint: "compose the prompt in $EDITOR (alt+e)",
        kind: Command::Editor,
    },
    Entry {
        command: "/image",
        aliases: &["/img"],
        hint: "attach an image to the next message - /image <path> (alt+v)",
        kind: Command::Image,
    },
    // -- agent features --------------------------------------------------------
    Entry {
        command: "/memory",
        aliases: &["/mem"],
        hint: "browse stored memories (alt+n)",
        kind: Command::Memory,
    },
    Entry {
        command: "/panel",
        aliases: &[],
        hint: "convene the agent board - /panel <topic> (alt+b)",
        kind: Command::Panel,
    },
    Entry {
        command: "/ab",
        aliases: &[],
        hint: "compare minimal vs high reasoning side by side - /ab <prompt> (alt+x)",
        kind: Command::Ab,
    },
    Entry {
        command: "/team",
        aliases: &["/parallel"],
        hint: "run three agents in parallel on one goal - /team <goal> (alt+shift+t)",
        kind: Command::Team,
    },
    Entry {
        command: "/improve",
        aliases: &[],
        hint: "review self-edit proposals (alt+i)",
        kind: Command::Improve,
    },
    Entry {
        command: "/improve-from-board",
        aliases: &[],
        hint: "stage the board's synthesis as a proposal (alt+shift+i)",
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
        hint: "show or hide the thinking indicator (alt+k)",
        kind: Command::Thinking,
    },
    Entry {
        command: "/focus",
        aliases: &[],
        hint: "show only the latest exchange (f2)",
        kind: Command::Focus,
    },
    Entry {
        command: "/density",
        aliases: &[],
        hint: "toggle compact density (ctrl+g)",
        kind: Command::Density,
    },
    Entry {
        command: "/themes",
        aliases: &["/theme"],
        hint: "switch colour theme (alt+t)",
        kind: Command::Themes,
    },
    Entry {
        command: "/hud",
        aliases: &[],
        hint: "toggle the performance hud (frame time, transcript stats) (alt+z)",
        kind: Command::Hud,
    },
    // -- output & general --------------------------------------------------------
    Entry {
        command: "/export",
        aliases: &[],
        hint: "write the session to markdown - /export html for a web page (alt+j)",
        kind: Command::Export,
    },
    Entry {
        command: "/share",
        aliases: &[],
        hint: "export a shareable copy (alt+shift+s)",
        kind: Command::Share,
    },
    Entry {
        command: "/unshare",
        aliases: &[],
        hint: "remove any shared copy (alt+shift+x)",
        kind: Command::Unshare,
    },
    Entry {
        command: "/init",
        aliases: &[],
        hint: "create or update AGENTS.md (alt+shift+n)",
        kind: Command::Init,
    },
    Entry {
        command: "/doctor",
        aliases: &[],
        hint: "run pre-flight diagnostics on the setup (alt+o)",
        kind: Command::Doctor,
    },
    Entry {
        command: "/help",
        aliases: &[],
        hint: "keyboard shortcuts and commands (f1)",
        kind: Command::Help,
    },
    Entry {
        command: "/exit",
        aliases: &["/quit", "/q"],
        hint: "quit nanomite (alt+q)",
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
    fn every_command_advertises_a_keybinding() {
        for entry in COMMANDS {
            let hint = entry.hint;
            let bound = ["alt+", "ctrl+", "shift+tab", "f1", "f2"]
                .iter()
                .any(|k| hint.contains(k));
            assert!(
                bound,
                "{} has no keybinding in its hint: {hint}",
                entry.command
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
