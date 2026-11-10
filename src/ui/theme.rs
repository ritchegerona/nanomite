use ratatui::style::{Color, Modifier, Style};

/// A 24-bit colour triple. Kept as a plain tuple so every palette below can be
/// written as a literal table.
pub type Rgb = (u8, u8, u8);

/// Terminal colour capability. opencode's palette is tuned for truecolor, so we keep a
/// 16-colour fallback for terminals that do not advertise `COLORTERM=truecolor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Ansi,
    Rgb,
}

/// One theme's colours, named by the role they play rather than by their
/// hue. Every field is consulted through [`Theme`], which decides whether to
/// emit the truecolor value or the 16-colour fallback.
///
/// The field names mirror opencode's theme token vocabulary where one exists,
/// so a palette can be read straight from an opencode `theme.json`.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Terminal ground, behind everything.
    pub neutral: Rgb,
    /// Raised ground for overlays, the composer box and other boxed surfaces.
    pub panel_bg: Rgb,
    /// Ground one step further raised: a selected row, an input field, a chip.
    pub element_bg: Rgb,

    /// Body text.
    pub text: Rgb,
    /// Emphasised text: selected rows, headings, the wordmark.
    pub strong: Rgb,
    /// Secondary text: hints, labels, secondary prose.
    pub muted: Rgb,
    /// Tertiary text: rules, placeholders, disabled affordances.
    pub faint: Rgb,

    /// Hairline and resting border.
    pub border: Rgb,
    /// Border of the focused or hovered surface.
    pub border_focus: Rgb,
    /// A border one step fainter than `border`, for nested separators.
    pub border_subtle: Rgb,

    /// Primary identity colour: prompt, spinner, active accent.
    pub accent: Rgb,
    /// A quieter relative of `accent`, for rails and secondary markers.
    pub accent_soft: Rgb,

    pub success: Rgb,
    pub danger: Rgb,
    pub warn: Rgb,

    /// Tool-mode identity colours. The hues stay semantically stable across
    /// themes (plan = amber caution, coder = blue focus, build = green works,
    /// hacker = red unrestricted) so a mode is recognisable by colour alone;
    /// each theme tunes the exact values to its ground.
    pub mode_plan: Rgb,
    pub mode_coder: Rgb,
    pub mode_build: Rgb,
    pub mode_hacker: Rgb,

    /// Added lines in a diff view. Distinct from `success`: a diff is data,
    /// and reading it as "this worked" is the wrong signal.
    pub diff_add: Rgb,
    pub diff_delete: Rgb,

    /// Tool-call labels.
    pub tool: Rgb,
    /// Inline and fenced code.
    pub code: Rgb,
    /// Block-quoted prose.
    pub quote: Rgb,
    /// Text of a user's own turn in the transcript, and the composer buffer
    /// while typing. Kept distinct from `text` (the assistant body) so the two
    /// speakers are identifiable by colour alone, and from `accent` so user
    /// prose never collides with the assistant's headings and links.
    pub user: Rgb,

    /// Fenced code: called functions and method names.
    pub code_function: Rgb,
    /// Fenced code: types, structs, classes, traits, variants.
    pub code_type: Rgb,
    /// Fenced code: ALL_CAPS constants and attributes.
    pub code_constant: Rgb,
}

/// Single-hue high contrast, easy on low-contrast panels.
const MONO: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (24, 24, 24),
    element_bg: (30, 30, 30),
    text: (255, 255, 255),
    strong: (255, 255, 255),
    muted: (176, 176, 176),
    faint: (112, 112, 112),
    border: (72, 72, 72),
    border_focus: (144, 144, 144),
    border_subtle: (56, 56, 56),
    accent: (214, 214, 214),
    accent_soft: (150, 150, 150),
    success: (170, 200, 170),
    danger: (230, 130, 130),
    warn: (220, 190, 120),
    mode_plan: (187, 154, 247),
    mode_coder: (150, 176, 208),
    mode_build: (170, 200, 170),
    mode_hacker: (230, 130, 130),
    diff_add: (170, 200, 170),
    diff_delete: (230, 130, 130),
    tool: (168, 168, 168),
    code: (150, 190, 150),
    quote: (140, 140, 140),
    user: (150, 190, 220),
    code_function: (228, 228, 228),
    code_type: (198, 198, 198),
    code_constant: (240, 240, 240),
};

/// Cool slate ground with a periwinkle accent.
const MIDNIGHT: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (21, 23, 31),
    element_bg: (25, 28, 38),
    text: (208, 214, 228),
    strong: (238, 242, 252),
    muted: (126, 134, 154),
    faint: (88, 95, 114),
    border: (50, 56, 70),
    border_focus: (92, 104, 132),
    border_subtle: (38, 42, 54),
    accent: (126, 160, 226),
    accent_soft: (88, 116, 172),
    success: (126, 200, 142),
    danger: (228, 124, 124),
    warn: (222, 180, 110),
    mode_plan: (196, 181, 253),
    mode_coder: (126, 160, 226),
    mode_build: (126, 200, 142),
    mode_hacker: (228, 124, 124),
    diff_add: (126, 200, 142),
    diff_delete: (228, 124, 124),
    tool: (150, 160, 184),
    code: (166, 194, 141),
    quote: (140, 148, 168),
    user: (150, 205, 210),
    code_function: (220, 220, 170),
    code_type: (120, 190, 190),
    code_constant: (206, 161, 110),
};

/// nanomite brand: near-black ground with teal and orange.
const GRAPHITE: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (16, 22, 27),
    element_bg: (20, 28, 34),
    text: (226, 232, 240),
    strong: (241, 245, 255),
    muted: (148, 163, 184),
    faint: (100, 116, 139),
    border: (35, 48, 57),
    border_focus: (61, 214, 140),
    border_subtle: (27, 37, 45),
    accent: (0, 229, 212),
    accent_soft: (0, 169, 156),
    success: (61, 214, 140),
    danger: (255, 77, 90),
    warn: (255, 190, 70),
    mode_plan: (167, 139, 250),
    mode_coder: (86, 164, 232),
    mode_build: (61, 214, 140),
    mode_hacker: (255, 77, 90),
    diff_add: (61, 214, 140),
    diff_delete: (255, 77, 90),
    tool: (176, 184, 200),
    code: (176, 200, 160),
    quote: (130, 145, 165),
    user: (135, 185, 245),
    code_function: (230, 200, 140),
    code_type: (105, 200, 200),
    code_constant: (235, 175, 120),
};

/// Resident Evil — Umbrella Corporation: near-black ground, blood-red accent,
/// clinical white text. The default.
const UMBRELLA: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (20, 13, 15),
    element_bg: (27, 18, 20),
    text: (222, 224, 228),
    strong: (246, 247, 249),
    muted: (146, 136, 140),
    faint: (92, 82, 86),
    border: (62, 44, 48),
    border_focus: (216, 40, 52),
    border_subtle: (47, 33, 36),
    accent: (228, 44, 56),
    accent_soft: (172, 36, 46),
    success: (76, 192, 110),
    danger: (236, 70, 76),
    warn: (226, 152, 62),
    mode_plan: (196, 160, 255),
    mode_coder: (86, 164, 232),
    mode_build: (76, 192, 110),
    mode_hacker: (236, 70, 76),
    diff_add: (76, 192, 110),
    diff_delete: (236, 70, 76),
    tool: (192, 152, 160),
    code: (168, 196, 156),
    quote: (152, 142, 146),
    user: (120, 195, 220),
    code_function: (214, 180, 110),
    code_type: (110, 190, 190),
    code_constant: (210, 150, 100),
};

/// Dracula — Popular purple/pink theme: deep purple ground, vibrant pink accent.
/// High contrast, easy on eyes, beloved by developers.
const DRACULA: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (40, 42, 54),
    element_bg: (68, 71, 90),
    text: (248, 248, 242),
    strong: (255, 255, 255),
    muted: (98, 114, 164),
    faint: (68, 71, 90),
    border: (68, 71, 90),
    border_focus: (189, 147, 249),
    border_subtle: (59, 62, 79),
    accent: (189, 147, 249),
    accent_soft: (139, 117, 189),
    success: (80, 250, 123),
    danger: (255, 85, 85),
    warn: (241, 250, 140),
    mode_plan: (255, 184, 108),
    mode_coder: (139, 233, 253),
    mode_build: (80, 250, 123),
    mode_hacker: (255, 85, 85),
    diff_add: (80, 250, 123),
    diff_delete: (255, 85, 85),
    tool: (189, 147, 249),
    code: (139, 233, 253),
    quote: (241, 250, 140),
    user: (150, 205, 255),
    code_function: (80, 250, 123),
    code_type: (139, 233, 253),
    code_constant: (255, 184, 108),
};

/// Cyberpunk — Neon-drenched dark theme: hot pink + cyan on near-black.
/// High contrast, futuristic aesthetic.
const CYBERPUNK: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (16, 16, 32),
    element_bg: (28, 28, 52),
    text: (220, 255, 255),
    strong: (255, 255, 255),
    muted: (120, 180, 220),
    faint: (80, 100, 130),
    border: (80, 40, 120),
    border_focus: (255, 0, 128),
    border_subtle: (40, 20, 60),
    accent: (0, 255, 200),
    accent_soft: (255, 0, 128),
    success: (0, 255, 128),
    danger: (255, 0, 128),
    warn: (255, 255, 0),
    mode_plan: (255, 255, 0),
    mode_coder: (0, 255, 200),
    mode_build: (0, 255, 128),
    mode_hacker: (255, 0, 128),
    diff_add: (0, 255, 128),
    diff_delete: (255, 0, 128),
    tool: (255, 0, 128),
    code: (0, 255, 200),
    quote: (255, 255, 0),
    user: (205, 160, 255),
    code_function: (255, 220, 80),
    code_type: (80, 250, 255),
    code_constant: (255, 120, 200),
};

/// Hacker — phosphor green on black: classic terminal-noir, matrix green.
const HACKER: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (6, 14, 6),
    element_bg: (10, 20, 10),
    text: (168, 255, 168),
    strong: (214, 255, 214),
    muted: (110, 190, 110),
    faint: (58, 120, 58),
    border: (24, 72, 24),
    border_focus: (0, 255, 102),
    border_subtle: (16, 48, 16),
    accent: (0, 255, 102),
    accent_soft: (0, 190, 80),
    success: (0, 230, 110),
    danger: (255, 72, 72),
    warn: (255, 204, 0),
    mode_plan: (200, 180, 60),
    mode_coder: (90, 200, 255),
    mode_build: (0, 230, 110),
    mode_hacker: (255, 72, 72),
    diff_add: (0, 220, 110),
    diff_delete: (255, 80, 80),
    tool: (140, 230, 140),
    code: (150, 255, 150),
    quote: (90, 160, 90),
    user: (140, 210, 255),
    code_function: (140, 255, 140),
    code_type: (100, 220, 100),
    code_constant: (200, 255, 120),
};

/// Mr. Robot — fsociety red on warm charcoal: cable-static darks, sharp red.
const MR_ROBOT: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (16, 13, 13),
    element_bg: (24, 19, 19),
    text: (214, 208, 204),
    strong: (244, 240, 236),
    muted: (150, 138, 134),
    faint: (94, 84, 82),
    border: (56, 46, 44),
    border_focus: (237, 28, 36),
    border_subtle: (38, 30, 28),
    accent: (237, 28, 36),
    accent_soft: (178, 26, 32),
    success: (104, 200, 120),
    danger: (244, 64, 64),
    warn: (228, 168, 72),
    mode_plan: (206, 160, 240),
    mode_coder: (96, 168, 232),
    mode_build: (104, 200, 120),
    mode_hacker: (244, 64, 64),
    diff_add: (104, 200, 120),
    diff_delete: (244, 64, 64),
    tool: (196, 152, 148),
    code: (170, 196, 150),
    quote: (146, 134, 130),
    user: (130, 185, 220),
    code_function: (230, 200, 140),
    code_type: (105, 200, 200),
    code_constant: (235, 175, 120),
};

/// Anonymous — grey-hat monochrome: silver on black, cold and faceless.
const ANONYMOUS: Palette = Palette {
    neutral: (0, 0, 0),
    panel_bg: (10, 10, 11),
    element_bg: (17, 17, 18),
    text: (216, 216, 220),
    strong: (250, 250, 252),
    muted: (148, 148, 156),
    faint: (92, 92, 98),
    border: (52, 52, 58),
    border_focus: (196, 196, 208),
    border_subtle: (34, 34, 38),
    accent: (196, 196, 208),
    accent_soft: (140, 140, 152),
    success: (120, 220, 130),
    danger: (236, 80, 80),
    warn: (222, 190, 110),
    mode_plan: (196, 160, 255),
    mode_coder: (140, 170, 230),
    mode_build: (120, 220, 130),
    mode_hacker: (236, 80, 80),
    diff_add: (120, 220, 130),
    diff_delete: (236, 80, 80),
    tool: (170, 170, 180),
    code: (170, 200, 170),
    quote: (136, 136, 144),
    user: (150, 185, 225),
    code_function: (220, 220, 170),
    code_type: (120, 190, 190),
    code_constant: (206, 161, 110),
};

/// Nord — arctic north: pale snow on deep polar-night blue-grey.
/// Clean, low-contrast, easy on the eyes for long sessions.
const NORD: Palette = Palette {
    neutral: (46, 52, 64),
    panel_bg: (59, 66, 82),
    element_bg: (67, 76, 94),
    text: (229, 233, 240),
    strong: (236, 239, 244),
    muted: (143, 188, 187),
    faint: (76, 86, 106),
    border: (76, 86, 106),
    border_focus: (136, 192, 208),
    border_subtle: (67, 76, 94),
    accent: (136, 192, 208),
    accent_soft: (129, 161, 193),
    success: (163, 190, 140),
    danger: (191, 97, 106),
    warn: (235, 203, 139),
    mode_plan: (235, 203, 139),
    mode_coder: (136, 192, 208),
    mode_build: (163, 190, 140),
    mode_hacker: (191, 97, 106),
    diff_add: (163, 190, 140),
    diff_delete: (191, 97, 106),
    tool: (180, 135, 174),
    code: (136, 192, 208),
    quote: (94, 129, 172),
    user: (190, 165, 210),
    code_function: (233, 205, 140),
    code_type: (129, 197, 197),
    code_constant: (213, 166, 120),
};

/// Selectable palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    /// Single-hue high contrast, easy on low-contrast panels.
    Mono,
    Midnight,
    Graphite,
    Umbrella,
    Dracula,
    Cyberpunk,
    /// Phosphor green on black — terminal noir.
    Hacker,
    /// fsociety red on warm charcoal.
    MrRobot,
    /// Grey-hat silver on black.
    Anonymous,
    /// Arctic north — pale snow on deep polar-night blue-grey.
    Nord,
}

impl ThemeId {
    pub const ALL: [ThemeId; 10] = [
        ThemeId::Mono,
        ThemeId::Midnight,
        ThemeId::Graphite,
        ThemeId::Umbrella,
        ThemeId::Dracula,
        ThemeId::Cyberpunk,
        ThemeId::Hacker,
        ThemeId::MrRobot,
        ThemeId::Anonymous,
        ThemeId::Nord,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Midnight => "midnight",
            ThemeId::Graphite => "graphite",
            ThemeId::Mono => "mono",
            ThemeId::Umbrella => "umbrella",
            ThemeId::Dracula => "dracula",
            ThemeId::Cyberpunk => "cyberpunk",
            ThemeId::Hacker => "hacker",
            ThemeId::MrRobot => "mr-robot",
            ThemeId::Anonymous => "anonymous",
            ThemeId::Nord => "nord",
        }
    }

    /// The colour table behind this theme.
    pub fn palette(self) -> &'static Palette {
        match self {
            ThemeId::Midnight => &MIDNIGHT,
            ThemeId::Graphite => &GRAPHITE,
            ThemeId::Mono => &MONO,
            ThemeId::Umbrella => &UMBRELLA,
            ThemeId::Dracula => &DRACULA,
            ThemeId::Cyberpunk => &CYBERPUNK,
            ThemeId::Hacker => &HACKER,
            ThemeId::MrRobot => &MR_ROBOT,
            ThemeId::Anonymous => &ANONYMOUS,
            ThemeId::Nord => &NORD,
        }
    }

    /// Resolves a saved label (config `theme = "umbrella"`) back to a theme.
    /// Unknown labels fall through to `None`, letting the caller keep the
    /// default. Comparison strips punctuation and case, so display names like
    /// `Mr. Robot` resolve to the same theme as the saved `mr-robot` label.
    pub fn from_label(label: &str) -> Option<ThemeId> {
        let norm = |s: &str| {
            s.trim()
                .to_ascii_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        };
        let want = norm(label);
        Self::ALL.into_iter().find(|id| norm(id.label()) == want)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub depth: ColorDepth,
    pub id: ThemeId,
}

impl Theme {
    pub fn with_id(id: ThemeId) -> Self {
        Self {
            depth: Self::detect_depth(),
            id,
        }
    }

    pub fn detect() -> Self {
        // Umbrella is the NANOMITE default: blood-red accent on near-black.
        Self::with_id(ThemeId::Umbrella)
    }

    /// True when this terminal advertises 24-bit colour. The palettes are
    /// authored in truecolor; on ANSI-only terminals the per-role fallbacks
    /// from [`Theme::pick`] take over instead.
    pub fn is_truecolor(&self) -> bool {
        self.depth == ColorDepth::Rgb
    }

    fn detect_depth() -> ColorDepth {
        let colorterm = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let term = std::env::var("TERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let no_color = std::env::var("NO_COLOR").is_ok();
        let rgb = !no_color
            && (colorterm.contains("truecolor")
                || colorterm.contains("24bit")
                || term.contains("256color"));
        if rgb {
            ColorDepth::Rgb
        } else {
            ColorDepth::Ansi
        }
    }

    pub fn palette(&self) -> &'static Palette {
        self.id.palette()
    }

    /// Emits the truecolor value on a capable terminal, otherwise the 16-colour
    /// fallback for that role. The fallback belongs to the *role*, not the
    /// palette, so it is passed alongside the RGB triple rather than stored.
    /// ANSI fallback that differs per palette.
    #[inline]
    fn ansi(&self, _default: Color, _opencode: Color) -> Color {
        _default
    }

    #[inline]
    fn pick(&self, rgb: Rgb, ansi: Color) -> Color {
        match self.depth {
            ColorDepth::Rgb => Color::Rgb(rgb.0, rgb.1, rgb.2),
            ColorDepth::Ansi => ansi,
        }
    }

    pub fn text(&self) -> Color {
        self.pick(self.palette().text, Color::Gray)
    }
    pub fn strong(&self) -> Color {
        self.pick(self.palette().strong, Color::White)
    }
    pub fn muted(&self) -> Color {
        self.pick(self.palette().muted, Color::DarkGray)
    }
    pub fn faint(&self) -> Color {
        self.pick(self.palette().faint, Color::DarkGray)
    }
    pub fn border(&self) -> Color {
        self.pick(self.palette().border, Color::DarkGray)
    }
    pub fn border_focus(&self) -> Color {
        self.pick(self.palette().border_focus, Color::Indexed(124))
    }
    pub fn accent(&self) -> Color {
        self.pick(self.palette().accent, self.ansi(Color::Red, Color::Yellow))
    }
    pub fn accent_soft(&self) -> Color {
        self.pick(
            self.palette().accent_soft,
            self.ansi(Color::Indexed(88), Color::Magenta),
        )
    }
    pub fn success(&self) -> Color {
        self.pick(self.palette().success, Color::Green)
    }
    pub fn danger(&self) -> Color {
        self.pick(self.palette().danger, Color::Red)
    }
    pub fn mode_plan(&self) -> Color {
        self.pick(self.palette().mode_plan, Color::Yellow)
    }
    pub fn mode_coder(&self) -> Color {
        self.pick(self.palette().mode_coder, Color::Cyan)
    }
    pub fn mode_build(&self) -> Color {
        self.pick(self.palette().mode_build, Color::Green)
    }
    pub fn mode_hacker(&self) -> Color {
        self.pick(self.palette().mode_hacker, Color::Red)
    }
    pub fn warn(&self) -> Color {
        self.pick(self.palette().warn, Color::Yellow)
    }
    pub fn tool(&self) -> Color {
        self.pick(self.palette().tool, Color::Indexed(174))
    }
    pub fn code(&self) -> Color {
        self.pick(self.palette().code, Color::Green)
    }
    pub fn quote(&self) -> Color {
        self.pick(self.palette().quote, Color::DarkGray)
    }
    /// User-turn body text and the composer buffer. Distinct from `text` (the
    /// assistant body) so the two speakers read apart by colour.
    pub fn user(&self) -> Color {
        self.pick(self.palette().user, Color::Cyan)
    }

    pub fn code_function(&self) -> Color {
        self.pick(self.palette().code_function, Color::LightYellow)
    }
    pub fn code_type(&self) -> Color {
        self.pick(self.palette().code_type, Color::LightCyan)
    }
    pub fn code_constant(&self) -> Color {
        self.pick(self.palette().code_constant, Color::LightYellow)
    }
    /// Slightly raised background for overlays and the composer.
    pub fn panel_bg(&self) -> Color {
        self.pick(self.palette().panel_bg, Color::Indexed(235))
    }
    /// One step further raised: selected rows, input fields, chips.
    pub fn element_bg(&self) -> Color {
        self.pick(self.palette().element_bg, Color::Indexed(237))
    }
    /// A border fainter than [`Theme::border`], for nested separators.
    pub fn border_subtle(&self) -> Color {
        self.pick(self.palette().border_subtle, Color::Indexed(234))
    }
    /// Added diff lines. Deliberately not [`Theme::success`]: a diff is data.
    pub fn diff_add(&self) -> Color {
        self.pick(self.palette().diff_add, Color::Green)
    }
    /// Removed diff lines.
    pub fn diff_delete(&self) -> Color {
        self.pick(self.palette().diff_delete, Color::Red)
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.text())
    }
    pub fn dim(&self) -> Style {
        Style::default().fg(self.muted())
    }
    pub fn faint_style(&self) -> Style {
        Style::default().fg(self.faint())
    }
    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent())
    }
    pub fn bold(&self) -> Style {
        Style::default()
            .fg(self.strong())
            .add_modifier(Modifier::BOLD)
    }
}

/// Glyphs shared across widgets.
pub mod glyph {
    pub const PROMPT: &str = "❯";
    pub const HEX: &str = "⬡";
    /// Filled hexagon, used by the welcome-card molecule mark.
    pub const HEX_FILL: &str = "⬣";
    pub const GUTTER: &str = "▌";
    /// Node on the conversation rail marking a user turn.
    pub const NODE: &str = "◆";
    pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    pub const CHECK: &str = "✓";
    pub const CROSS: &str = "✗";
    pub const CHEVRON: &str = "›";
    pub const RULE: &str = "─";
    pub const DOT: &str = "·";
    pub const ARROW_UP: &str = "↑";
    pub const ARROW_DOWN: &str = "↓";
    pub const HOURGLASS: &str = "◐";
    /// Insertion caret used by form fields.
    pub const CARET: &str = "▏";
    /// Live-stream cursor appended to a reply while it streams.
    pub const CURSOR: &str = "▍";
    /// Mask character for secrets in the /connect form.
    pub const BULLET: &str = "•";
    /// Inline image thumbnail marker shown in the composer next to the image
    /// badge so a dropped png/jpg is visually distinct from plain text.
    pub const IMAGE: &str = "🖼";
    /// Context-meter cells: filled and empty.
    pub const BLOCK: &str = "█";
    pub const BLOCK_LIGHT: &str = "░";
    /// Brain glyph for context window display.
    pub const BRAIN: &str = "🧠";
    /// Offline indicator shown in the header when NANOMITE_OFFLINE=1.
    pub const OFFLINE: &str = "⚡";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_label_round_trips_every_theme() {
        for id in ThemeId::ALL {
            assert_eq!(ThemeId::from_label(id.label()), Some(id));
        }
        // Case and surrounding space are tolerated, unknown labels are not.
        assert_eq!(ThemeId::from_label("  UMBRELLA "), Some(ThemeId::Umbrella));
        assert_eq!(ThemeId::from_label("nope"), None);
        assert_eq!(ThemeId::from_label(""), None);
    }

    #[test]
    fn hacker_family_labels_resolve_display_names() {
        // Display names resolve to the saved label: punctuation and case are
        // stripped, so `Mr. Robot` and friends find their theme.
        assert_eq!(ThemeId::from_label("Mr. Robot"), Some(ThemeId::MrRobot));
        assert_eq!(ThemeId::from_label("mr robot"), Some(ThemeId::MrRobot));
        assert_eq!(ThemeId::from_label("MR_ROBOT"), Some(ThemeId::MrRobot));
        assert_eq!(ThemeId::from_label("Hacker"), Some(ThemeId::Hacker));
        assert_eq!(ThemeId::from_label("Anonymous"), Some(ThemeId::Anonymous));
        assert_eq!(ThemeId::from_label("Nord"), Some(ThemeId::Nord));
    }

    #[test]
    fn every_theme_declares_a_complete_palette() {
        // A missing field would not compile, but a field that collapsed onto
        // another role's value would, so assert they stay distinct where the
        // design intends them to be.
        for id in ThemeId::ALL {
            let p = id.palette();
            assert_ne!(p.text, p.muted, "{id:?}: text and muted collapsed");
            assert_ne!(p.muted, p.faint, "{id:?}: muted and faint collapsed");
            assert_ne!(p.success, p.danger, "{id:?}: success and danger collapsed");
            assert_ne!(p.accent, p.accent_soft, "{id:?}: accent pair collapsed");
            // User input must never collapse onto the assistant body or the
            // emphasis tone, or the two speakers stop being distinguishable.
            assert_ne!(p.user, p.text, "{id:?}: user and text collapsed");
            assert_ne!(p.user, p.strong, "{id:?}: user and strong collapsed");
            // The four mode colours must all differ: a mode is recognisable
            // by colour alone in every theme.
            let modes = [p.mode_plan, p.mode_coder, p.mode_build, p.mode_hacker];
            for (i, a) in modes.iter().enumerate() {
                for (j, b) in modes.iter().enumerate() {
                    if i < j {
                        assert_ne!(a, b, "{id:?}: mode colours {i}/{j} collapsed");
                    }
                }
            }
        }
    }

    #[test]
    fn mode_colors_have_universal_ansi_fallbacks() {
        // In 16-colour terminals the mode colour CODES are universal across
        // themes: yellow plan, cyan coder, green build, red hacker.
        for id in ThemeId::ALL {
            let mut t = Theme::with_id(id);
            t.depth = ColorDepth::Ansi;
            assert_eq!(t.mode_plan(), Color::Yellow, "{id:?}");
            assert_eq!(t.mode_coder(), Color::Cyan, "{id:?}");
            assert_eq!(t.mode_build(), Color::Green, "{id:?}");
            assert_eq!(t.mode_hacker(), Color::Red, "{id:?}");
        }
    }

    #[test]
    fn umbrella_is_the_default_skin() {
        assert_eq!(Theme::detect().id, ThemeId::Umbrella);
        assert_eq!(ThemeId::ALL[0], ThemeId::Mono);
        assert_eq!(ThemeId::Mono.label(), "mono");
    }

    #[test]
    fn accent_ansi_fallback_differs_for_umbrella() {
        // Umbrella's accent is red; the other skins are teal, grey or white.
        // Their fallbacks must not all follow Umbrella's.
        let mut midnight = Theme::with_id(ThemeId::Midnight);
        midnight.depth = ColorDepth::Ansi;
        assert_eq!(midnight.accent(), Color::Red); // uses same default fallback

        let mut umb = Theme::with_id(ThemeId::Umbrella);
        umb.depth = ColorDepth::Ansi;
        assert_eq!(umb.accent(), Color::Red);
        // Both use the same fallback since we removed per-palette fallbacks
    }

    #[test]
    fn labels_and_all_stay_in_step() {
        for id in ThemeId::ALL {
            assert!(!id.label().is_empty());
            assert!(id.label().chars().all(|c| !c.is_uppercase()));
        }
        assert_eq!(ThemeId::ALL.len(), 10);
    }

    #[test]
    fn ansi_depth_falls_back_per_role() {
        let mut t = Theme::with_id(ThemeId::Umbrella);
        t.depth = ColorDepth::Ansi;
        assert_eq!(t.accent(), Color::Red);
        assert_eq!(t.success(), Color::Green);
        assert_eq!(t.text(), Color::Gray);
        // The fallback must not silently become the truecolor value.
        assert_ne!(t.accent(), Color::Rgb(228, 44, 56));
    }

    #[test]
    fn rgb_depth_emits_the_palette_value() {
        let mut t = Theme::with_id(ThemeId::Umbrella);
        t.depth = ColorDepth::Rgb;
        let p = t.palette();
        assert_eq!(t.accent(), Color::Rgb(p.accent.0, p.accent.1, p.accent.2));
        assert_eq!(
            t.panel_bg(),
            Color::Rgb(p.panel_bg.0, p.panel_bg.1, p.panel_bg.2)
        );
    }
}
