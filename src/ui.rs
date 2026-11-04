use std::f64::consts::PI;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Line as CLine, Points};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::dsp::{F_MAX, F_MIN, vu_pos};
use crate::theme::Theme;
use crate::{App, ScopeMode, View};

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const LED_MIN_DB: f32 = -60.0;

pub fn draw(f: &mut Frame, app: &mut App, dt: f32) {
    let dt = dt * app.speed();
    let [main, footer_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

    match app.view {
        View::Dashboard => dashboard(f, main, app, dt),
        View::Spectrum => spectrum_panel(f, main, app, dt),
        View::Scope => scope_panel(f, main, app, dt),
        View::Meters => meters_view(f, main, app),
    }
    footer(f, footer_area, app);
    if app.help {
        help_popup(f, app.theme());
    }
    if let Some(cockpit) = &app.theme().cockpit {
        // Done last so canvases and popups can't punch through to the terminal.
        for cell in &mut f.buffer_mut().content {
            if cell.bg == Color::Reset {
                cell.bg = cockpit.screen;
            }
        }
    }
}

fn panel<'a>(title: &'a str, th: &Theme) -> Block<'a> {
    let title_style = Style::new().fg(th.text).add_modifier(Modifier::BOLD);
    let (border_type, title_style) = match &th.cockpit {
        Some(c) => (BorderType::Plain, title_style.bg(c.placard)),
        None => (BorderType::Rounded, title_style),
    };
    Block::bordered()
        .border_type(border_type)
        .border_style(Style::new().fg(th.border))
        .title(Span::styled(title, title_style))
}

fn dashboard(f: &mut Frame, area: Rect, app: &mut App, dt: f32) {
    let (left, right) = if area.width >= 100 {
        let [l, r] = Layout::horizontal([Constraint::Min(40), Constraint::Length(34)]).areas(area);
        (l, Some(r))
    } else {
        (area, None)
    };

    // Without a right column the correlation meter goes under the LEDs.
    let corr_h = if right.is_none() { 3 } else { 0 };
    let [spec, scope, leds, corr] = Layout::vertical([
        Constraint::Percentage(55),
        Constraint::Min(5),
        Constraint::Length(5),
        Constraint::Length(corr_h),
    ])
    .areas(left);
    spectrum_panel(f, spec, app, dt);
    scope_panel(f, scope, app, dt);
    led_meters(f, leds, app);

    if let Some(right) = right {
        let [vl, vr, corr] =
            Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2), Constraint::Length(3)]).areas(right);
        vu_needle(f, vl, app, 0);
        vu_needle(f, vr, app, 1);
        correlation_panel(f, corr, app);
    } else {
        correlation_panel(f, corr, app);
    }
}

fn meters_view(f: &mut Frame, area: Rect, app: &mut App) {
    let led_h = (area.height / 3).clamp(5, 11);
    let [top, bottom, corr] =
        Layout::vertical([Constraint::Min(6), Constraint::Length(led_h), Constraint::Length(3)]).areas(area);
    let [vl, vr] = Layout::horizontal([Constraint::Ratio(1, 2); 2]).areas(top);
    vu_needle(f, vl, app, 0);
    vu_needle(f, vr, app, 1);
    led_meters(f, bottom, app);
    correlation_panel(f, corr, app);
}

// ───────────────────────────── Spectrum ─────────────────────────────

struct BarGeom {
    n: usize,
    bar_w: u16,
    slot: u16,
    offset: u16,
}

impl BarGeom {
    fn new(width: u16, n: usize) -> Self {
        let n = n.clamp(1, width.max(1) as usize);
        let slot = (width as usize / n).max(1) as u16;
        let bar_w = if slot >= 2 { slot - 1 } else { 1 };
        let total = n as u16 * slot - (slot - bar_w);
        Self { n, bar_w, slot, offset: width.saturating_sub(total) / 2 }
    }
}

fn spectrum_panel(f: &mut Frame, area: Rect, app: &mut App, dt: f32) {
    let th = app.theme();
    let block = panel(" Spectrum ", th)
        .title_bottom(Line::styled(format!(" {} bars ", app.bars_label()), Style::new().fg(th.border)).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let labels = inner.height >= 6;
    let bars_area = Rect { height: inner.height - labels as u16, ..inner };
    let geom = BarGeom::new(bars_area.width, app.bar_count(bars_area.width));
    if app.frozen {
        app.spectrum.resize(geom.n);
    } else {
        app.spectrum.update(&app.l, &app.r, geom.n, app.sens_db, dt);
    }

    let buf = f.buffer_mut();
    draw_bars(buf, bars_area, &geom, &app.spectrum.bars, &app.spectrum.peaks, th);
    if labels {
        let row = Rect { y: inner.bottom() - 1, height: 1, ..inner };
        draw_freq_labels(buf, row, &geom, th);
    }
}

fn draw_bars(buf: &mut Buffer, area: Rect, g: &BarGeom, bars: &[f32], peaks: &[f32], th: &Theme) {
    let h = area.height as usize;
    let row_colors: Vec<Color> = (0..h).map(|row| th.grad((row as f32 + 0.5) / h as f32)).collect();

    for (i, (&v, &p)) in bars.iter().zip(peaks).enumerate().take(g.n) {
        let x0 = area.x + g.offset + i as u16 * g.slot;
        let eighths = (v.clamp(0.0, 1.0) * (h * 8) as f32).round() as usize;

        for row in 0..h {
            let fill = eighths.saturating_sub(row * 8).min(8);
            if fill == 0 {
                break;
            }
            let y = area.bottom() - 1 - row as u16;
            for dx in 0..g.bar_w {
                if let Some(cell) = buf.cell_mut((x0 + dx, y)) {
                    cell.set_char(BLOCKS[fill]).set_fg(row_colors[row]);
                }
            }
        }

        // Peak marker, drawn only where it floats above the bar.
        if p > 0.02 {
            let level = p.clamp(0.0, 0.999) * h as f32;
            let row = level as usize;
            if eighths <= row * 8 {
                let frac = level - row as f32;
                let ch = if frac < 0.33 { '▁' } else if frac < 0.66 { '─' } else { '▔' };
                let y = area.bottom() - 1 - row as u16;
                for dx in 0..g.bar_w {
                    if let Some(cell) = buf.cell_mut((x0 + dx, y)) {
                        cell.set_char(ch).set_fg(th.peak);
                    }
                }
            }
        }
    }
}

fn draw_freq_labels(buf: &mut Buffer, row: Rect, g: &BarGeom, th: &Theme) {
    let marks = [
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (5000.0, "5k"),
        (10000.0, "10k"),
    ];
    let span = (g.n as u16 * g.slot) as f32;
    let mut next_free = row.x;
    for (hz, label) in marks {
        let frac = (hz / F_MIN).ln() / (F_MAX / F_MIN).ln();
        let center = row.x + g.offset + (frac * span) as u16;
        let start = center.saturating_sub(label.len() as u16 / 2);
        if start < next_free || start + label.len() as u16 > row.right() {
            continue;
        }
        buf.set_string(start, row.y, label, Style::new().fg(th.border));
        next_free = start + label.len() as u16 + 1;
    }
}

// ───────────────────────────── Oscilloscope ─────────────────────────────

fn scope_panel(f: &mut Frame, area: Rect, app: &mut App, dt: f32) {
    let th = app.theme();
    let title = match app.scope_mode {
        ScopeMode::Overlay => " Oscilloscope · L+R ",
        ScopeMode::Split => " Oscilloscope · L / R ",
        ScopeMode::Lissajous => " Goniometer · stereo XY ",
    };
    let legend = Line::from(vec![
        Span::styled(" ● L ", Style::new().fg(th.wave_l)),
        Span::styled("● R ", Style::new().fg(th.wave_r)),
    ])
    .right_aligned();
    let block = panel(title, th).title(legend);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 2 || inner.height < 2 || app.l.is_empty() {
        return;
    }

    // Auto-gain: follow the signal peak quickly, release slowly.
    if !app.frozen {
        let recent = app.l.len().saturating_sub(2048);
        let pk = app.l[recent..].iter().chain(&app.r[recent..]).fold(0f32, |a, &x| a.max(x.abs()));
        app.scope_peak = pk.max(app.scope_peak * (-dt / 1.5).exp());
    }
    let gain = 0.9 / app.scope_peak.max(0.05);

    match app.scope_mode {
        ScopeMode::Lissajous if inner.height >= 6 => {
            let [plot, corr] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            goniometer(f, plot, app, gain);
            draw_correlation(f.buffer_mut(), corr, app);
        }
        ScopeMode::Lissajous => goniometer(f, inner, app, gain),
        mode => waveform(f, inner, app, gain, mode == ScopeMode::Split),
    }
}

fn waveform(f: &mut Frame, area: Rect, app: &App, gain: f32, split: bool) {
    let th = app.theme();
    let (l, r) = (&app.l, &app.r);
    let len = l.len();
    // Sweep speed: a faster sweep shows a shorter stretch of the signal.
    let win = ((1024.0 / app.speed()) as usize).min(len / 2);

    // Trigger on the most recent rising zero crossing so the trace stands still.
    let search_end = len - win;
    let search_start = search_end.saturating_sub(1024).max(1);
    let start = (search_start..search_end)
        .rev()
        .find(|&i| {
            let m0 = l[i - 1] + r[i - 1];
            let m1 = l[i] + r[i];
            m0 < 0.0 && m1 >= 0.0
        })
        .unwrap_or(search_end);

    let pts = (area.width as usize * 2).max(2);
    let sample = |buf: &[f32], k: usize| -> f64 {
        let idx = start + k * win / pts;
        (buf[idx] * gain).clamp(-1.0, 1.0) as f64
    };
    let trace = |buf: &[f32], center: f64, scale: f64| -> Vec<(f64, f64)> {
        (0..pts).map(|k| (k as f64, center + sample(buf, k) * scale)).collect()
    };

    let (tl, tr, y_bounds) = if split {
        (trace(l, 1.0, 0.95), trace(r, -1.0, 0.95), [-2.0, 2.0])
    } else {
        (trace(l, 0.0, 1.0), trace(r, 0.0, 1.0), [-1.0, 1.0])
    };

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, (pts - 1) as f64])
        .y_bounds(y_bounds)
        .paint(|ctx| {
            let xmax = (pts - 1) as f64;
            let centers: &[f64] = if split { &[1.0, -1.0] } else { &[0.0] };
            for &c in centers {
                ctx.draw(&CLine::new(0.0, c, xmax, c, th.dim));
            }
            ctx.layer();
            polyline(ctx, &tr, th.wave_r);
            ctx.layer();
            polyline(ctx, &tl, th.wave_l);
        });
    f.render_widget(canvas, area);
}

fn polyline(ctx: &mut Context, pts: &[(f64, f64)], color: Color) {
    for w in pts.windows(2) {
        ctx.draw(&CLine::new(w[0].0, w[0].1, w[1].0, w[1].1, color));
    }
}

fn goniometer(f: &mut Frame, area: Rect, app: &App, gain: f32) {
    let th = app.theme();
    // Terminal cells are ~1:2, so a square plot is twice as wide as tall.
    let w = area.width.min(area.height * 2);
    let h = (w / 2).max(1);
    let sq = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };

    let s = std::f32::consts::FRAC_1_SQRT_2;
    let n = app.l.len().min(2048);
    let off = app.l.len() - n;
    let coords: Vec<(f64, f64)> = (off..app.l.len())
        .map(|i| {
            let (l, r) = (app.l[i] * gain, app.r[i] * gain);
            (((r - l) * s).clamp(-1.0, 1.0) as f64, ((l + r) * s).clamp(-1.0, 1.0) as f64)
        })
        .collect();

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(|ctx| {
            ctx.draw(&CLine::new(0.0, -1.0, 0.0, 1.0, th.dim));
            ctx.draw(&CLine::new(-1.0, -1.0, 1.0, 1.0, th.dim));
            ctx.draw(&CLine::new(-1.0, 1.0, 1.0, -1.0, th.dim));
            ctx.print(-0.95, 0.9, Span::styled("L", Style::new().fg(th.wave_l)));
            ctx.print(0.9, 0.9, Span::styled("R", Style::new().fg(th.wave_r)));
            ctx.print(0.03, 0.9, Span::styled("M", Style::new().fg(th.border)));
            ctx.layer();
            ctx.draw(&Points { coords: &coords, color: th.wave_l });
        });
    f.render_widget(canvas, sq);
}

// ───────────────────────────── Level meters ─────────────────────────────

fn led_meters(f: &mut Frame, area: Rect, app: &App) {
    let th = app.theme();
    let block = panel(" Peak level · dBFS ", th);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 2 || inner.width < 20 {
        return;
    }

    let rows_per_ch = ((inner.height - 1) / 2).max(1);
    let label_w = 2u16;
    let readout_w = 9u16;
    let seg_w = inner.width - label_w - readout_w;
    let buf = f.buffer_mut();

    for c in 0..2 {
        let y0 = inner.y + c as u16 * rows_per_ch;
        let peak = app.meters.peak_db[c];
        let hold = app.meters.hold_db[c];
        let hold_seg = ((hold - LED_MIN_DB) / -LED_MIN_DB * seg_w as f32) as i32;

        for dy in 0..rows_per_ch {
            let y = y0 + dy;
            if dy == rows_per_ch / 2 {
                buf.set_string(inner.x, y, ["L", "R"][c], Style::new().fg(th.text).add_modifier(Modifier::BOLD));
            }
            for i in 0..seg_w {
                let db = LED_MIN_DB + (i as f32 + 0.5) / seg_w as f32 * -LED_MIN_DB;
                let zone = if db >= -6.0 { 1.0 } else if db >= -18.0 { 0.6 } else { 0.1 };
                let color = if db <= peak {
                    th.grad(zone)
                } else if i as i32 == hold_seg && hold > LED_MIN_DB {
                    th.peak
                } else {
                    th.dim
                };
                if let Some(cell) = buf.cell_mut((inner.x + label_w + i, y)) {
                    cell.set_char('▌').set_fg(color);
                }
            }
            if dy == rows_per_ch / 2 {
                let x = inner.x + label_w + seg_w;
                if app.meters.clip_t[c] > 0.0 {
                    buf.set_string(x, y, "   CLIP  ", Style::new().fg(Color::White).bg(th.grad(1.0)).add_modifier(Modifier::BOLD));
                } else {
                    let text = if peak <= -99.0 { "    -∞ dB".to_string() } else { format!("{:>6.1} dB", peak) };
                    buf.set_string(x, y, text, Style::new().fg(th.text));
                }
            }
        }
    }

    // dB scale.
    let y = inner.y + 2 * rows_per_ch;
    if y < inner.bottom() {
        let mut next_free = 0u16;
        for db in [-60, -48, -36, -24, -18, -12, -6, -3, 0] {
            let label = db.to_string();
            let pos = ((db as f32 - LED_MIN_DB) / -LED_MIN_DB * seg_w as f32) as u16;
            let start = (label_w + pos).saturating_sub(label.len() as u16 / 2).min(label_w + seg_w - label.len() as u16);
            if start < next_free {
                continue;
            }
            buf.set_string(inner.x + start, y, &label, Style::new().fg(th.border));
            next_free = start + label.len() as u16 + 1;
        }
    }
}

fn correlation_panel(f: &mut Frame, area: Rect, app: &App) {
    let block = panel(" Phase correlation ", app.theme());
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height > 0 {
        let row = Rect { y: inner.y + (inner.height - 1) / 2, height: 1, ..inner };
        draw_correlation(f.buffer_mut(), row, app);
    }
}

/// One-line meter: `-1 ───────┼━━━━█── +1  +0.62`. Negative (red) means
/// the channels cancel in mono; near +1 means (almost) mono.
fn draw_correlation(buf: &mut Buffer, row: Rect, app: &App) {
    let th = app.theme();
    let (corr, active) = (app.meters.corr, app.meters.corr_active);
    let readout_w = 7u16;
    if row.width < 16 {
        return;
    }
    let track_w = row.width - 6 - readout_w;
    let x0 = row.x + 3;
    let label = Style::new().fg(th.border);
    buf.set_string(row.x, row.y, "-1", label);
    buf.set_string(x0 + track_w + 1, row.y, "+1", label);

    let pos = |v: f32| (((v + 1.0) / 2.0) * (track_w - 1) as f32).round() as u16;
    let center = pos(0.0);
    let marker = pos(corr.clamp(-1.0, 1.0));
    let color = if corr < 0.0 {
        th.grad(1.0)
    } else if corr < 0.3 {
        th.grad(0.6)
    } else {
        th.grad(0.0)
    };

    for i in 0..track_w {
        let (ch, fg) = if active && i == marker {
            ('█', color)
        } else if i == center {
            ('┼', th.border)
        } else if active && i >= center.min(marker) && i <= center.max(marker) {
            ('━', color)
        } else {
            ('─', th.dim)
        };
        if let Some(cell) = buf.cell_mut((x0 + i, row.y)) {
            cell.set_char(ch).set_fg(fg);
        }
    }

    let readout = if active { format!("{corr:>+7.2}") } else { "      —".into() };
    buf.set_string(row.right() - readout_w, row.y, readout, Style::new().fg(if active { color } else { th.border }));
}

fn vu_needle(f: &mut Frame, area: Rect, app: &App, ch: usize) {
    let th = app.theme();
    let vu = app.meters.vu_db[ch];
    let readout = Line::styled(format!(" {:+.1} VU ", vu.clamp(-99.0, 9.9)), Style::new().fg(th.text)).right_aligned();
    let block = panel(if ch == 0 { " VU · Left " } else { " VU · Right " }, th).title(readout);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 8 || inner.height < 3 {
        return;
    }

    // Size the coordinate system so the arc stays circular whatever the
    // panel's shape: each braille dot is (cell_w/2) × (cell_h/4).
    let (cols, rows) = (inner.width as f64, inner.height as f64);
    // The dial occupies roughly x ∈ [-1, 1], y ∈ [-0.1, 1.25]; centre it.
    let unit = (2.1 / (2.0 * cols)).max(1.4 / (4.0 * rows));
    let half_w = cols * unit;
    let span_y = 4.0 * rows * unit;
    let y0 = -0.1 - (span_y - 1.4) / 2.0;
    let y1 = y0 + span_y;
    let cell_w = 2.0 * half_w / cols;

    let angle = |pos: f32| (145.0 - pos.clamp(-0.05, 1.05) as f64 * 110.0) * PI / 180.0;
    let at = |pos: f32, r: f64| {
        let a = angle(pos);
        (r * a.cos(), r * a.sin())
    };
    let zero = vu_pos(0.0);
    let red = th.grad(1.0);
    let needle = app.meters.needle[ch];
    let compact = inner.width < 24;

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-half_w, half_w])
        .y_bounds([y0, y1])
        .paint(move |ctx| {
            // Scale arc: neutral up to 0 VU, red above.
            let segs = 80;
            for i in 0..segs {
                let (p0, p1) = (i as f32 / segs as f32, (i + 1) as f32 / segs as f32);
                let (a, b) = (at(p0, 0.9), at(p1, 0.9));
                ctx.draw(&CLine::new(a.0, a.1, b.0, b.1, if p0 >= zero { red } else { th.text }));
            }
            for db in [-20.0, -10.0, -7.0, -5.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0] {
                let p = vu_pos(db);
                let (a, b) = (at(p, 0.9), at(p, 1.0));
                ctx.draw(&CLine::new(a.0, a.1, b.0, b.1, if db >= 0.0 { red } else { th.text }));
            }
            let labels: &[f32] = if compact { &[-20.0, 0.0, 3.0] } else { &[-20.0, -10.0, -5.0, -3.0, 0.0, 3.0] };
            for &db in labels {
                let text = if db > 0.0 { format!("+{db}") } else { format!("{db}") };
                let (x, y) = at(vu_pos(db), 1.12);
                let x = x - text.chars().count() as f64 * cell_w / 2.0;
                ctx.print(x, y, Span::styled(text, Style::new().fg(if db >= 0.0 { red } else { th.border })));
            }
            ctx.print(-cell_w, 0.42, Span::styled("VU", Style::new().fg(th.border).add_modifier(Modifier::BOLD)));
            ctx.layer();
            let (tip_x, tip_y) = at(needle, 0.97);
            ctx.draw(&CLine::new(0.0, 0.0, tip_x, tip_y, th.needle));
        });
    f.render_widget(canvas, inner);
}

// ───────────────────────────── Chrome ─────────────────────────────

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let th = app.theme();
    let key = |k: &'static str| Span::styled(k, Style::new().fg(th.grad(0.0)).add_modifier(Modifier::BOLD));
    // Border grey is unreadable on a placard-coloured footer.
    let txt_fg = if th.cockpit.is_some() { th.text } else { th.border };
    let txt = |t: String| Span::styled(t, Style::new().fg(txt_fg));
    let view = match app.view {
        View::Dashboard => "dashboard",
        View::Spectrum => "spectrum",
        View::Scope => "scope",
        View::Meters => "meters",
    };
    let mut spans = vec![
        Span::styled(" ♪ ", Style::new().fg(th.grad(0.6))),
        Span::styled(app.source.clone(), Style::new().fg(th.text)),
        txt(format!("  │ {view} │ gain {:+.0} dB │ speed {}× │ {} ", app.sens_db, app.speed(), th.name)),
    ];
    if app.frozen {
        spans.push(Span::styled(" FROZEN ", Style::new().fg(Color::Black).bg(th.grad(0.6))));
    }
    spans.extend([
        txt("  ".into()),
        key("1-4"),
        txt(" view  ".into()),
        key("h"),
        txt(" help  ".into()),
        key("q"),
        txt(" quit".into()),
    ]);
    if let Some(c) = &th.cockpit {
        f.buffer_mut().set_style(area, Style::new().bg(c.placard));
    }
    f.render_widget(Line::from(spans), area);
}

fn help_popup(f: &mut Frame, th: &Theme) {
    let rows = [
        ("1 2 3 4 / Tab", "dashboard · spectrum · scope · meters"),
        ("+ / - / ↑ ↓", "gain (spectrum & VU sensitivity)"),
        ("0", "reset gain"),
        (", / . / ← →", "graph speed: slower · faster"),
        ("s", "spectrum bar count"),
        ("m", "scope mode: overlay · split · goniometer"),
        ("c", "colour theme"),
        ("p / space", "freeze display"),
        ("h / ?", "toggle this help"),
        ("q / Esc", "quit"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!(" {k:<15}"), Style::new().fg(th.grad(0.0)).add_modifier(Modifier::BOLD)),
                Span::styled(*d, Style::new().fg(th.text)),
            ])
        })
        .collect();

    let area = f.area();
    let (w, h) = (60.min(area.width), (rows.len() as u16 + 2).min(area.height));
    let popup = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(panel(" Keys ", th)), popup);
}
