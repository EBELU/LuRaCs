use egui::{Color32, Stroke};

// ─── Flavor ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavor {
    Latte,
    Frappe,
    Macchiato,
    #[default]
    Mocha,
}

impl Flavor {
    pub const ALL: [Self; 4] = [Self::Latte, Self::Frappe, Self::Macchiato, Self::Mocha];

    pub const fn is_dark(self) -> bool {
        !matches!(self, Self::Latte)
    }

    pub const fn palette(self) -> Palette {
        match self {
            Self::Latte => Palette::LATTE,
            Self::Frappe => Palette::FRAPPE,
            Self::Macchiato => Palette::MACCHIATO,
            Self::Mocha => Palette::MOCHA,
        }
    }
}

// ─── Palette ────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    // Accent colors
    pub rosewater: Color32,
    pub flamingo: Color32,
    pub pink: Color32,
    pub mauve: Color32,
    pub red: Color32,
    pub maroon: Color32,
    pub peach: Color32,
    pub yellow: Color32,
    pub green: Color32,
    pub teal: Color32,
    pub sky: Color32,
    pub sapphire: Color32,
    pub blue: Color32,
    pub lavender: Color32,

    // Monochromatic colors
    pub text: Color32,
    pub subtext1: Color32,
    pub subtext0: Color32,
    pub overlay2: Color32,
    pub overlay1: Color32,
    pub overlay0: Color32,
    pub surface2: Color32,
    pub surface1: Color32,
    pub surface0: Color32,
    pub base: Color32,
    pub mantle: Color32,
    pub crust: Color32,
}

impl Palette {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32::from_rgb(r, g, b)
    }

    pub const LATTE: Self = Self {
        // Fill with official Latte RGB values
        rosewater: Self::rgb(220, 138, 120),
        flamingo: Self::rgb(221, 120, 120),
        pink: Self::rgb(234, 118, 203),
        mauve: Self::rgb(136, 57, 239),
        red: Self::rgb(210, 15, 57),
        maroon: Self::rgb(230, 69, 83),
        peach: Self::rgb(254, 100, 11),
        yellow: Self::rgb(223, 142, 29),
        green: Self::rgb(64, 160, 43),
        teal: Self::rgb(23, 146, 153),
        sky: Self::rgb(4, 165, 229),
        sapphire: Self::rgb(32, 159, 181),
        blue: Self::rgb(30, 102, 245),
        lavender: Self::rgb(114, 135, 253),

        text: Self::rgb(76, 79, 105),
        subtext1: Self::rgb(92, 95, 119),
        subtext0: Self::rgb(108, 111, 133),
        overlay2: Self::rgb(124, 127, 147),
        overlay1: Self::rgb(140, 143, 161),
        overlay0: Self::rgb(156, 160, 176),
        surface2: Self::rgb(172, 176, 190),
        surface1: Self::rgb(188, 192, 204),
        surface0: Self::rgb(204, 208, 218),
        base: Self::rgb(239, 241, 245),
        mantle: Self::rgb(230, 233, 239),
        crust: Self::rgb(220, 224, 232),
    };

    pub const FRAPPE: Self = Self {
        rosewater: Self::rgb(242, 213, 207),
        flamingo: Self::rgb(238, 190, 190),
        pink: Self::rgb(244, 184, 228),
        mauve: Self::rgb(202, 158, 230),
        red: Self::rgb(231, 130, 132),
        maroon: Self::rgb(234, 153, 156),
        peach: Self::rgb(239, 159, 118),
        yellow: Self::rgb(229, 200, 144),
        green: Self::rgb(166, 209, 137),
        teal: Self::rgb(129, 200, 190),
        sky: Self::rgb(153, 209, 219),
        sapphire: Self::rgb(133, 193, 220),
        blue: Self::rgb(140, 170, 238),
        lavender: Self::rgb(186, 187, 241),

        text: Self::rgb(198, 208, 245),
        subtext1: Self::rgb(181, 191, 226),
        subtext0: Self::rgb(165, 173, 206),
        overlay2: Self::rgb(148, 156, 187),
        overlay1: Self::rgb(131, 139, 167),
        overlay0: Self::rgb(115, 121, 148),
        surface2: Self::rgb(98, 104, 128),
        surface1: Self::rgb(81, 87, 109),
        surface0: Self::rgb(65, 69, 89),
        base: Self::rgb(48, 52, 70),
        mantle: Self::rgb(41, 44, 60),
        crust: Self::rgb(35, 38, 52),
    };

    pub const MACCHIATO: Self = Self {
        rosewater: Self::rgb(244, 219, 214),
        flamingo: Self::rgb(240, 198, 198),
        pink: Self::rgb(245, 189, 230),
        mauve: Self::rgb(198, 160, 246),
        red: Self::rgb(237, 135, 150),
        maroon: Self::rgb(238, 153, 160),
        peach: Self::rgb(245, 169, 127),
        yellow: Self::rgb(238, 212, 159),
        green: Self::rgb(166, 218, 149),
        teal: Self::rgb(139, 213, 202),
        sky: Self::rgb(145, 215, 227),
        sapphire: Self::rgb(125, 196, 228),
        blue: Self::rgb(138, 173, 244),
        lavender: Self::rgb(183, 189, 248),

        text: Self::rgb(202, 211, 245),
        subtext1: Self::rgb(184, 192, 224),
        subtext0: Self::rgb(165, 173, 203),
        overlay2: Self::rgb(147, 154, 183),
        overlay1: Self::rgb(128, 135, 162),
        overlay0: Self::rgb(110, 115, 141),
        surface2: Self::rgb(91, 96, 120),
        surface1: Self::rgb(73, 77, 100),
        surface0: Self::rgb(54, 58, 79),
        base: Self::rgb(36, 39, 58),
        mantle: Self::rgb(30, 32, 48),
        crust: Self::rgb(24, 25, 38),
    };

    pub const MOCHA: Self = Self {
        rosewater: Self::rgb(245, 224, 220),
        flamingo: Self::rgb(242, 205, 205),
        pink: Self::rgb(245, 194, 231),
        mauve: Self::rgb(203, 166, 247),
        red: Self::rgb(243, 139, 168),
        maroon: Self::rgb(235, 160, 172),
        peach: Self::rgb(250, 179, 135),
        yellow: Self::rgb(249, 226, 175),
        green: Self::rgb(166, 227, 161),
        teal: Self::rgb(148, 226, 213),
        sky: Self::rgb(137, 220, 235),
        sapphire: Self::rgb(116, 199, 236),
        blue: Self::rgb(137, 180, 250),
        lavender: Self::rgb(180, 190, 254),

        text: Self::rgb(205, 214, 244),
        subtext1: Self::rgb(186, 194, 222),
        subtext0: Self::rgb(166, 173, 200),
        overlay2: Self::rgb(147, 153, 178),
        overlay1: Self::rgb(127, 132, 156),
        overlay0: Self::rgb(108, 112, 134),
        surface2: Self::rgb(88, 91, 112),
        surface1: Self::rgb(69, 71, 90),
        surface0: Self::rgb(49, 50, 68),
        base: Self::rgb(30, 30, 46),
        mantle: Self::rgb(24, 24, 37),
        crust: Self::rgb(17, 17, 27),
    };
}

// ─── Catppuccin Theme ───────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct CatppuccinTheme {
    pub flavor: Flavor,
}

impl CatppuccinTheme {
    pub const fn new(flavor: Flavor) -> Self {
        Self { flavor }
    }

    pub const fn palette(&self) -> Palette {
        self.flavor.palette()
    }

    pub fn visuals(&self) -> egui::Visuals {
        let p = self.palette();

        let mut visuals = if self.flavor.is_dark() {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.dark_mode = self.flavor.is_dark();

        // General colors
        visuals.override_text_color = Some(p.text);
        visuals.panel_fill = p.base;
        visuals.window_fill = p.base;
        visuals.extreme_bg_color = p.crust;
        visuals.faint_bg_color = p.mantle;
        visuals.code_bg_color = p.mantle;

        // Selection
        visuals.selection.bg_fill = p.surface1;
        visuals.selection.stroke = Stroke::new(1.0, p.text);

        // Hyperlinks
        visuals.hyperlink_color = p.blue;

        // Window and popup borders
        visuals.window_stroke = Stroke::new(1.0, p.surface0);

        // Widget states
        visuals.widgets.noninteractive.bg_fill = p.base;
        visuals.widgets.noninteractive.fg_stroke.color = p.text;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.surface0);

        visuals.widgets.inactive.bg_fill = p.surface0;
        visuals.widgets.inactive.weak_bg_fill = p.surface0;
        visuals.widgets.inactive.fg_stroke.color = p.text;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.surface1);

        visuals.widgets.hovered.bg_fill = p.surface1;
        visuals.widgets.hovered.weak_bg_fill = p.surface1;
        visuals.widgets.hovered.fg_stroke.color = p.text;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.blue);

        visuals.widgets.active.bg_fill = p.surface2;
        visuals.widgets.active.weak_bg_fill = p.surface2;
        visuals.widgets.active.fg_stroke.color = p.text;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.mauve);

        visuals.widgets.open.bg_fill = p.surface1;
        visuals.widgets.open.weak_bg_fill = p.surface1;
        visuals.widgets.open.fg_stroke.color = p.text;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.blue);

        visuals
    }

    pub fn style(&self) -> egui::Style {
        egui::Style {
            visuals: self.visuals(),
            ..Default::default()
        }
    }
}

impl From<Flavor> for CatppuccinTheme {
    fn from(flavor: Flavor) -> Self {
        Self::new(flavor)
    }
}
