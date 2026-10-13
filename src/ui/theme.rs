use ratatui::style::{Color, Modifier, Style};

/// Terminal colour capability. opencode's palette is tuned for truecolor, so we keep a
/// 16-colour fallback for terminals that do not advertise `COLORTERM=truecolor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Ansi,
    Rgb,
}

/// Selectable palettes, mirroring opencode's `/themes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    /// Cool slate + periwinkle accent.
    Midnight,
    /// nanomite brand: near-black ground with teal and orange.
    Graphite,
    /// Single-hue high contrast, easy on low-contrast panels.
    Mono,
    /// Resident Evil — Umbrella Corporation: near-black ground, blood-red accent,
    /// clinical white text. The default.
    Umbrella,
}

impl ThemeId {
    pub const ALL: [ThemeId; 4] = [
        ThemeId::Midnight,
        ThemeId::Graphite,
        ThemeId::Mono,
        ThemeId::Umbrella,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Midnight => "midnight",
            ThemeId::Graphite => "graphite",
            ThemeId::Mono => "mono",
            ThemeId::Umbrella => "umbrella",
        }
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
        // Umbrella Corp is the default skin.
        Self::with_id(ThemeId::Umbrella)
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

    /// Palette-aware lookup: midnight, graphite, mono, umbrella, then the 16-colour
    /// fallback.
    #[inline]
    fn themed(
        &self,
        mid: (u8, u8, u8),
        graphite: (u8, u8, u8),
        mono: (u8, u8, u8),
        umbrella: (u8, u8, u8),
        ansi: Color,
    ) -> Color {
        let rgb = match self.id {
            ThemeId::Midnight => mid,
            ThemeId::Graphite => graphite,
            ThemeId::Mono => mono,
            ThemeId::Umbrella => umbrella,
        };
        self.pick(rgb, ansi)
    }

    #[inline]
    fn pick(&self, rgb: (u8, u8, u8), ansi: Color) -> Color {
        match self.depth {
            ColorDepth::Rgb => Color::Rgb(rgb.0, rgb.1, rgb.2),
            ColorDepth::Ansi => ansi,
        }
    }

    pub fn text(&self) -> Color {
        self.themed(
            (208, 214, 228),
            (226, 232, 240),
            (255, 255, 255),
            (222, 224, 228),
            Color::Gray,
        )
    }
    pub fn strong(&self) -> Color {
        self.themed(
            (238, 242, 252),
            (241, 245, 255),
            (255, 255, 255),
            (246, 247, 249),
            Color::White,
        )
    }
    pub fn muted(&self) -> Color {
        self.themed(
            (126, 134, 154),
            (148, 163, 184),
            (176, 176, 176),
            (146, 136, 140),
            Color::DarkGray,
        )
    }
    pub fn faint(&self) -> Color {
        self.themed(
            (88, 95, 114),
            (100, 116, 139),
            (112, 112, 112),
            (92, 82, 86),
            Color::DarkGray,
        )
    }
    pub fn border(&self) -> Color {
        self.themed(
            (50, 56, 70),
            (35, 48, 57),
            (72, 72, 72),
            (62, 44, 48),
            Color::DarkGray,
        )
    }
    pub fn border_focus(&self) -> Color {
        self.themed(
            (92, 104, 132),
            (61, 214, 140),
            (144, 144, 144),
            (216, 40, 52),
            Color::Indexed(124),
        )
    }
    pub fn accent(&self) -> Color {
        self.themed(
            (126, 160, 226),
            (0, 229, 212),
            (214, 214, 214),
            (228, 44, 56),
            Color::Red,
        )
    }
    pub fn accent_soft(&self) -> Color {
        self.themed(
            (88, 116, 172),
            (0, 169, 156),
            (150, 150, 150),
            (172, 36, 46),
            Color::Indexed(88),
        )
    }

    /// Linear RGB blend (`t` = 0 → a, 1 → b). Terminals cannot composite, so
    /// "transparency" is simulated by washing the hue into the panel ground.
    pub fn blend(a: Color, b: Color, t: f32) -> Color {
        match (a, b) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let f = |x: u8, y: u8| {
                    (x as f32 + (y as f32 - x as f32) * t)
                        .round()
                        .clamp(0.0, 255.0) as u8
                };
                Color::Rgb(f(r1, r2), f(g1, g2), f(b1, b2))
            }
            _ => {
                if t >= 0.5 {
                    b
                } else {
                    a
                }
            }
        }
    }

    /// User hue per palette: green identity, tuned to each theme's ground.
    fn user_hue(&self) -> Color {
        self.themed(
            (44, 104, 70),
            (30, 96, 80),
            (60, 92, 60),
            (34, 88, 56),
            Color::Indexed(22),
        )
    }

    /// Agent hue per palette: blue identity, tuned to each theme's ground.
    fn agent_hue(&self) -> Color {
        self.themed(
            (46, 68, 118),
            (34, 74, 112),
            (62, 82, 108),
            (30, 54, 96),
            Color::Indexed(17),
        )
    }

    /// Translucent user wash: the hue washed over the panel ground (no border).
    pub fn user_bubble(&self) -> Color {
        Self::blend(self.panel_bg(), self.user_hue(), 0.55)
    }

    /// Frosted edge for the user bubble's padding rows (fake blur falloff).
    pub fn user_bubble_soft(&self) -> Color {
        Self::blend(self.panel_bg(), self.user_hue(), 0.22)
    }

    /// Translucent agent wash: the hue washed over the panel ground (no border).
    pub fn agent_bubble(&self) -> Color {
        Self::blend(self.panel_bg(), self.agent_hue(), 0.50)
    }

    /// Frosted edge for the agent bubble's padding rows (fake blur falloff).
    pub fn agent_bubble_soft(&self) -> Color {
        Self::blend(self.panel_bg(), self.agent_hue(), 0.20)
    }
    pub fn success(&self) -> Color {
        self.themed(
            (126, 200, 142),
            (61, 214, 140),
            (170, 200, 170),
            (76, 192, 110),
            Color::Green,
        )
    }
    pub fn danger(&self) -> Color {
        self.themed(
            (228, 124, 124),
            (255, 77, 90),
            (230, 130, 130),
            (236, 70, 76),
            Color::Red,
        )
    }
    pub fn warn(&self) -> Color {
        self.themed(
            (222, 180, 110),
            (255, 190, 70),
            (220, 190, 120),
            (226, 152, 62),
            Color::Yellow,
        )
    }
    pub fn tool(&self) -> Color {
        self.themed(
            (150, 160, 184),
            (176, 184, 200),
            (168, 168, 168),
            (192, 152, 160),
            Color::Indexed(174),
        )
    }
    pub fn code(&self) -> Color {
        self.themed(
            (166, 194, 141),
            (176, 200, 160),
            (150, 190, 150),
            (168, 196, 156),
            Color::Green,
        )
    }
    pub fn quote(&self) -> Color {
        self.themed(
            (140, 148, 168),
            (130, 145, 165),
            (140, 140, 140),
            (152, 142, 146),
            Color::DarkGray,
        )
    }
    /// Slightly raised background for overlays and the composer.
    pub fn panel_bg(&self) -> Color {
        let rgb = match self.id {
            ThemeId::Midnight => (21, 23, 31),
            ThemeId::Graphite => (16, 22, 27),
            ThemeId::Mono => (24, 24, 24),
            ThemeId::Umbrella => (20, 13, 15),
        };
        self.pick(rgb, Color::Indexed(235))
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
    pub const GUTTER: &str = "▌";
    pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    pub const CHECK: &str = "✓";
    pub const CROSS: &str = "✗";
    pub const CHEVRON: &str = "›";
    pub const RULE: &str = "─";
    pub const DOT: &str = "·";
    pub const ARROW_UP: &str = "↑";
    pub const ARROW_DOWN: &str = "↓";
    pub const HOURGLASS: &str = "◐";
}
