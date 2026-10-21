use ratatui::style::Color;

pub struct Theme {
    pub name: &'static str,
    /// Gradient stops: low → mid → high (green/yellow/red in the classic look).
    pub stops: [(u8, u8, u8); 3],
    pub wave_l: Color,
    pub wave_r: Color,
    pub border: Color,
    pub dim: Color,
    pub text: Color,
    pub needle: Color,
    pub peak: Color,
}

impl Theme {
    /// Colour at `t` (0..1) along the theme's gradient.
    pub fn grad(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let (a, b, u) = if t < 0.6 {
            (self.stops[0], self.stops[1], t / 0.6)
        } else {
            (self.stops[1], self.stops[2], (t - 0.6) / 0.4)
        };
        let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * u).round() as u8;
        Color::Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
    }
}

pub static THEMES: [Theme; 3] = [
    Theme {
        name: "classic",
        stops: [(40, 200, 60), (230, 220, 40), (240, 50, 40)],
        wave_l: Color::Rgb(80, 230, 120),
        wave_r: Color::Rgb(60, 170, 240),
        border: Color::Rgb(90, 115, 100),
        dim: Color::Rgb(38, 52, 42),
        text: Color::Rgb(180, 200, 185),
        needle: Color::Rgb(245, 245, 235),
        peak: Color::Rgb(240, 240, 240),
    },
    Theme {
        name: "amber",
        stops: [(160, 70, 0), (255, 165, 20), (255, 235, 130)],
        wave_l: Color::Rgb(255, 176, 0),
        wave_r: Color::Rgb(255, 110, 30),
        border: Color::Rgb(140, 90, 30),
        dim: Color::Rgb(60, 36, 10),
        text: Color::Rgb(255, 200, 120),
        needle: Color::Rgb(255, 240, 200),
        peak: Color::Rgb(255, 250, 220),
    },
    Theme {
        name: "neon",
        stops: [(0, 220, 255), (180, 60, 255), (255, 40, 160)],
        wave_l: Color::Rgb(0, 255, 230),
        wave_r: Color::Rgb(255, 60, 200),
        border: Color::Rgb(110, 60, 170),
        dim: Color::Rgb(40, 25, 62),
        text: Color::Rgb(210, 190, 255),
        needle: Color::Rgb(255, 255, 255),
        peak: Color::Rgb(255, 255, 255),
    },
];
