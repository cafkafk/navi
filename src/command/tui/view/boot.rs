use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
    Frame,
};

use crate::command::tui::model::{App, WiredPhantom};
use crate::command::tui::view::quotes::LAIN_QUOTES;
use rand::Rng;

const NAVI_HEADER: &str = r#"
███╗   ██╗ █████╗ ██╗   ██╗██╗
████╗  ██║██╔══██╗██║   ██║██║
██╔██╗ ██║███████║██║   ██║██║
██║╚██╗██║██╔══██║╚██╗ ██╔╝██║
██║ ╚████║██║  ██║ ╚████╔╝ ██║
╚═╝  ╚═══╝╚═╝  ╚═╝  ╚═══╝  ╚═╝
"#;

const LOGO: &str = r#"
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣤⣶⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠻⠿⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⠀⠀⠀⠀⢀⣠⣴⣶⣿⣿⣿⣿⣿⣿⣿⣶⣦⣄⡀⠀⠀⠀⠀⣀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣾⣿⣿⡆⠀⢀⣴⣿⣿⣿⣿⠿⠟⠛⠛⠛⠻⠿⣿⣿⣿⣿⣦⡀⠀⢰⣿⣿⣷⣄⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⣿⣿⣿⠟⠉⠀⣴⣿⣿⣿⠟⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠻⣿⣿⣿⣦⠀⠉⠻⢿⣿⣿⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⣿⣿⡿⠋⠀⠀⠀⣼⣿⣿⡿⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⢿⣿⣿⣧⠀⠀⠀⠙⢿⣿⣿⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣿⣿⡿⠉⠀⠀⠀⠀⢰⣿⣿⣿⠁⠀⠀⠀⠀⣤⣾⣿⣿⣿⣷⣦⠀⠀⠀⠀⠈⣿⣿⣿⡆⠀⠀⠀⠀⠈⠻⣿⣿⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣼⣿⡿⡏⠀⠀⠀⠀⠀⠀⣼⣿⣿⡏⠀⠀⠀⠀⣼⣿⣿⣿⣿⣿⣿⣿⣷⠀⠀⠀⠀⢸⣿⣿⣷⠀⠀⠀⠀⠀⠀⢹⣿⣿⣧⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⡇⠀⠀⠀⠀⠀⠀⢿⣿⣿⡇⠀⠀⠀⠀⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⢸⣿⣿⣿⠀⠀⠀⠀⠀⠀⢠⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠘⣿⣿⣷⡄⠀⠀⠀⠀⠀⢸⣿⣿⣷⠀⠀⠀⠀⠘⢿⣿⣿⣿⣿⣿⡿⠃⠀⠀⠀⠀⣼⣿⣿⡏⠀⠀⠀⠀⠀⢀⣾⣿⣿⠏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣿⣿⣦⣄⠀⠀⠀⠀⢿⣿⣿⣇⠀⠀⠀⠀⠀⠉⠛⠛⠛⠉⠀⠀⠀⠀⠀⣸⣿⣿⣿⠁⠀⠀⠀⣠⣴⣿⣿⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣿⣿⣷⣤⡀⠀⠘⢿⣿⣿⣷⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣾⣿⣿⡿⠃⠀⢀⣠⣾⣿⣿⠿⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣤⡀⠈⠻⢿⣿⣿⣶⠀⠀⠻⣿⣿⣿⣷⣦⣄⠀⠀⠀⠀⠀⣠⣤⣾⣿⣿⡿⠏⠁⠀⣴⣿⣿⣿⠟⠉⢀⣤⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠘⢿⢿⠇⠀⠀⠀⠉⠛⠛⠀⠀⠀⠈⠙⠿⣿⣿⣿⣷⠀⠀⠀⣾⣿⣿⣿⠿⠁⠀⠀⠀⠀⠙⠛⠋⠀⠀⠀⠸⣿⣿⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⣿⣿⣿⠀⠀⠀⣿⣿⣿⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⠀⠀⠀⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⠀⠀⠀⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢰⣵⣷⣄⠀⠀⠀⠀⠀⢀⣿⣿⣿⠀⠀⠀⣿⣿⣿⡄⠀⠀⠀⠀⠀⣠⣾⣷⡆⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣿⣿⣷⣦⣄⣀⣤⣾⣿⡿⠋⠀⠀⠀⠘⣿⣿⣷⣤⣀⣠⣤⣾⣿⡿⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠙⠻⠿⣿⣿⡟⠛⠁⠀⠀⠀⠀⠀⠀⠈⠘⠿⢿⣿⣿⠻⠏⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
"#;

pub fn draw_boot(f: &mut Frame, app: &mut App) {
    let area = f.size();
    f.render_widget(BootWidget { app }, area);
}

struct BootWidget<'a> {
    app: &'a App,
}

impl<'a> Widget for BootWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let app = self.app;

        let cycle_len = 700;
        let cycle_idx = app.tick_count / cycle_len;
        let cycle_pos = app.tick_count % cycle_len;

        // INTERMISSION PHASE (500-700)
        // Disable intermission for cycle 4+ (steady state)
        if cycle_pos >= 500 && cycle_idx < 4 {
            // Determine style based on cycle index
            // Cycle 0, 1, 2 (Intermissions 1, 2, 3) -> Normal (Black BG)
            // Cycle 3 (Intermission 4) -> Red Mode
            let is_red_mode = cycle_idx >= 3;

            let bg_color = if is_red_mode {
                Color::Red
            } else {
                app.terminal_bg.unwrap_or(Color::Reset)
            };
            let text_color = if is_red_mode {
                Color::Black
            } else {
                Color::White
            };

            // 1. Clear / Fill Background
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    let cell = buf.get_mut(x, y);
                    cell.set_char(' ');
                    cell.set_style(Style::default().bg(bg_color).fg(text_color));
                }
            }

            // Helper to draw centered text with style
            let draw_centered = |text: &str, style: Style, buf: &mut Buffer| {
                let text_len = text.len() as u16;
                let x = area.width.saturating_sub(text_len) / 2;
                let y = area.height / 2;
                if x < area.width && y < area.height {
                    buf.set_string(x, y, text, style);
                }
            };

            let base_style = Style::default()
                .fg(text_color)
                .bg(bg_color)
                .add_modifier(Modifier::BOLD);

            if cycle_pos >= 520 && cycle_pos < 560 {
                draw_centered("PRESENT DAY", base_style, buf);
            } else if cycle_pos >= 580 && cycle_pos < 620 {
                draw_centered("PRESENT TIME", base_style, buf);
            } else if cycle_pos >= 640 && cycle_pos < 690 {
                let shake = (app.tick_count % 3) as i16 - 1;
                let text = "HAHAHAHAHA";
                let text_len = text.len() as u16;
                let base_x = (area.width.saturating_sub(text_len) / 2) as i16 + shake;
                let base_y = (area.height / 2) as i16 + shake;

                let muahaha_color = if is_red_mode {
                    Color::Black
                } else {
                    Color::Red
                };

                if base_x >= 0 && base_y >= 0 {
                    buf.set_string(
                        base_x as u16,
                        base_y as u16,
                        text,
                        base_style.fg(muahaha_color),
                    );
                }
            }
            return;
        }

        // NORMAL PHASE (0-500)
        let width = area.width as usize;
        let height = area.height as usize;

        // 1. Draw Background
        if cycle_idx < 3 {
            // Cycle 0, 1, 2: Game of Life Background
            let sim_width = app.game_of_life.width.min(width);
            let sim_height = app.game_of_life.height.min(height);

            for y in 0..sim_height {
                for x in 0..sim_width {
                    let cell = buf.get_mut(area.x + x as u16, area.y + y as u16);
                    if app.game_of_life.cells[y][x] {
                        cell.set_symbol("·");
                    } else {
                        cell.set_symbol(" ");
                    }
                    cell.set_style(Style::default().fg(Color::DarkGray));
                }
            }
        } else if cycle_idx == 3 {
            // Cycle 3: Aggressive Flashing "HAHAHAHAHA"
            let text = "HAHAHAHAHA";
            let text_len = text.len();

            // High density random scattering
            let count = 60;
            for i in 0..count {
                let seed = (app.tick_count as u64).wrapping_add(i * 99991);
                let rx = (seed % (width as u64).saturating_sub(text_len as u64).max(1)) as u16;
                let ry = ((seed / 100) % height as u64) as u16;

                // Randomize style slightly
                let is_bold = (seed % 2) == 0;
                let color = if (seed % 3) == 0 {
                    Color::Red
                } else {
                    Color::DarkGray
                };

                let style = Style::default().fg(color);
                let style = if is_bold {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                };

                buf.set_string(rx, ry, text, style);
            }
        } else {
            // Cycle 4+: Scrolling Text or Wired Phantoms

            if cycle_idx == 4 {
                // Cycle 4: Obsession Phase (Scrolling Text)
                let raw_text = "LETS ALL LOVE LAIN";
                let pattern = format!("{}   ", raw_text);
                let pat_len = pattern.len();
                let shift = (app.tick_count / 2) as usize;

                for y in 0..height {
                    for x in 0..width {
                        let char_idx = (x + y + shift) % pat_len;
                        let ch = pattern.chars().nth(char_idx).unwrap();

                        if ch != ' ' {
                            let cell = buf.get_mut(area.x + x as u16, area.y + y as u16);
                            cell.set_char(ch);
                            // Use darker grey for background text to be subtle
                            cell.set_style(Style::default().fg(Color::Rgb(60, 60, 60)));
                        }
                    }
                }
            } else {
                // Cycle 5+: Clear background to black
                for y in 0..height {
                    for x in 0..width {
                        let cell = buf.get_mut(area.x + x as u16, area.y + y as u16);
                        cell.set_char(' ');
                        cell.set_style(Style::default());
                    }
                }
            }
        }

        // 2. Main Draw Logic (Logo + Header)
        // Logo Dimming Logic: Cycle 5 fades logo out quickly (to show Phantoms "greyscale error" mode)
        let logo_dim = if cycle_idx >= 5 {
            let fade_ticks = 300.0; // 5 seconds fade out
            let start_tick = 5 * 700;
            let elapsed = app.tick_count.saturating_sub(start_tick) as f32;
            let progress = (elapsed / fade_ticks).min(1.0);
            1.0 - progress
        } else {
            1.0
        };

        // Phantom Dimming Logic: Hold for 20s, then fade out over 10s
        let phantom_dim = if cycle_idx >= 5 {
            let active_duration = 1200; // 20s
            let fade_duration = 600; // 10s
            let start_tick = 5 * 700;
            let elapsed = app.tick_count.saturating_sub(start_tick);

            if elapsed < active_duration {
                1.0
            } else {
                let fade_elapsed = elapsed - active_duration;
                let progress = (fade_elapsed as f32 / fade_duration as f32).min(1.0);
                1.0 - progress
            }
        } else {
            1.0
        };

        if logo_dim > 0.01 {
            let logo_line_count = LOGO.lines().count();
            let logo_height = logo_line_count as u16;
            let logo_width = LOGO.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;

            let navi_line_count = NAVI_HEADER.lines().filter(|l| !l.trim().is_empty()).count();
            let navi_height = navi_line_count as u16;
            let navi_width = NAVI_HEADER
                .lines()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0) as u16;

            let gap = 1;
            let total_content_height = navi_height + gap + logo_height;

            let center_y_start = (area.height.saturating_sub(total_content_height)) / 2;

            let logo_start_y = center_y_start + navi_height + gap;
            let center_y = logo_start_y;
            let center_x = (area.width.saturating_sub(logo_width)) / 2;

            let local_center_row = (logo_height as f32 - 1.0) / 2.0;
            let local_center_col = (logo_width as f32 - 1.0) / 2.0;

            let center_r = 45.0;
            let center_g = 25.0;
            let center_b = 55.0;

            let max_radius_x = logo_width as f32 * 0.50;
            let max_radius_y = logo_height as f32 * 0.45;
            let max_radius_x_sq = max_radius_x * max_radius_x;
            let max_radius_y_sq = max_radius_y * max_radius_y;

            // Lava Lamp Animation State
            // Cycle 0: Ramp up brightness/movement (0.0 to 1.0)
            // Cycle 1+: Full intensity (1.0)
            let ramp_progress = if cycle_idx == 0 {
                (cycle_pos as f32 / 500.0).min(1.0)
            } else {
                1.0
            };

            let time = app.tick_count as f32 * 0.03;

            // Render LOGO (Back Layer)
            let palette = [
                (0_f32, 255_f32, 255_f32),
                (0_f32, 150_f32, 255_f32),
                (0_f32, 0_f32, 255_f32),
                (138_f32, 43_f32, 226_f32),
                (255_f32, 0_f32, 255_f32),
                (255_f32, 20_f32, 147_f32),
            ];

            for (row, line_str) in LOGO.lines().enumerate() {
                let row_u16 = row as u16;
                let screen_y = center_y + row_u16;
                if screen_y >= area.height {
                    break;
                }

                let pos = (row as f32 * 0.1 + time) % palette.len() as f32;
                let idx1 = pos.floor() as usize;
                let idx2 = (idx1 + 1) % palette.len();
                let t = pos - pos.floor();

                let (r1, g1, b1) = palette[idx1];
                let (r2, g2, b2) = palette[idx2];

                // Mix with grayscale if ramping up
                let c_r = ((r1 + (r2 - r1) * t) * ramp_progress + (50.0 * (1.0 - ramp_progress)))
                    * logo_dim;
                let c_g = ((g1 + (g2 - g1) * t) * ramp_progress + (50.0 * (1.0 - ramp_progress)))
                    * logo_dim;
                let c_b = ((b1 + (b2 - b1) * t) * ramp_progress + (50.0 * (1.0 - ramp_progress)))
                    * logo_dim;

                let glitch_seed = (row as u64)
                    .wrapping_mul(app.tick_count)
                    .wrapping_add(app.tick_count);
                let is_glitch = (glitch_seed % 350) == 0;

                let (fg_r, fg_g, fg_b) = if is_glitch {
                    (255, 255, 255)
                } else {
                    (c_r as u8, c_g as u8, c_b as u8)
                };

                let mut chars = line_str.chars();
                for col in 0..logo_width {
                    let screen_x = center_x + col;
                    if screen_x >= area.width {
                        break;
                    }
                    let ch = chars.next().unwrap_or(' ');

                    // SDF
                    let dx = (col as f32 - local_center_col).abs();
                    let dy = (row as f32 - local_center_row).abs();
                    let norm_dist = (dx * dx / max_radius_x_sq) + (dy * dy / max_radius_y_sq);

                    let mut bg_color = None;
                    if norm_dist <= 1.0 {
                        let factor = (1.0 - norm_dist).max(0.0) * ramp_progress * logo_dim;

                        if let Some(Color::Rgb(br, bg, bb)) = app.terminal_bg {
                            let br = br as f32;
                            let bg = bg as f32;
                            let bb = bb as f32;
                            let r = (br * (1.0 - factor) + center_r * factor) as u8;
                            let g = (bg * (1.0 - factor) + center_g * factor) as u8;
                            let b = (bb * (1.0 - factor) + center_b * factor) as u8;
                            bg_color = Some(Color::Rgb(r, g, b));
                        } else {
                            // If terminal background is unknown (likely transparent or default),
                            // we cannot safely render a glow background without creating a stark border.
                            // So we disable the background glow in this case.
                            bg_color = None;
                        }
                    }

                    // Transparency / Tinting Logic
                    let cell = buf.get_mut(screen_x, screen_y);
                    let is_logo_empty = ch == ' ' || ch == '\u{2800}';

                    if is_logo_empty {
                        // If logo is empty, we tint the background (if it exists) with the glow
                        // This creates the overlay effect on top of scrolling text
                        if let Some(bg) = bg_color {
                            cell.set_bg(bg);
                        }
                    } else {
                        // Logo pixel exists, draw it normally
                        let mut style = Style::default().fg(Color::Rgb(fg_r, fg_g, fg_b));
                        if let Some(bg) = bg_color {
                            style = style.bg(bg);
                        }
                        if is_glitch {
                            style = style.add_modifier(Modifier::BOLD);
                        }

                        cell.set_char(ch).set_style(style);
                    }
                }
            }

            // Render NAVI Header (Front Layer - Dripping)
            let navi_center_x = (area.width.saturating_sub(navi_width)) / 2;
            let mut row_idx = 0;

            for line in NAVI_HEADER.lines().filter(|l| !l.trim().is_empty()) {
                let chars = line.chars();
                let mut x = navi_center_x;
                for ch in chars {
                    if x >= area.width {
                        break;
                    }

                    // Dripping Physics State Machine
                    let col_local = x.saturating_sub(navi_center_x);
                    let seed = (col_local as f64 * 12.34).sin();

                    let drip_offset = if cycle_idx == 0 {
                        0
                    } else if cycle_idx == 1 {
                        let t_cycle = cycle_pos as f64;
                        let start_delay = ((seed + 1.0) * 100.0).abs();
                        let g = 0.005 + ((seed * 123.0).sin().abs() * 0.005);
                        let active_time = (t_cycle - start_delay).max(0.0);
                        (0.5 * g * active_time.powf(2.0)) as u16
                    } else if cycle_idx == 2 {
                        let t_end = 500.0;
                        let start_delay = ((seed + 1.0) * 100.0).abs();
                        let g = 0.005 + ((seed * 123.0).sin().abs() * 0.005);
                        let active_time_end = (t_end - start_delay).max(0.0);
                        let max_depth = 0.5 * g * active_time_end.powf(2.0);

                        let t_cycle = cycle_pos as f64;
                        let progress = (t_cycle / 500.0).min(1.0);
                        (max_depth * (1.0 - progress)) as u16
                    } else {
                        0
                    };

                    // Lava Lamp Colors
                    let sx = x as f64 * 0.5;
                    let plasma_time = time as f64;
                    let sy = ((row_idx + drip_offset) as f64 * 0.2) - (plasma_time * 0.2);

                    let v1 = (sx + plasma_time * 0.1).sin();
                    let v2 = (sx * 0.5 + sy + plasma_time * 0.2).sin();
                    let v3 = ((sx + sy) * 0.3).cos();
                    let plasma = (v1 + v2 + v3) / 3.0;

                    let pi = std::f64::consts::PI;
                    let r_base = (plasma * pi).sin() * 100.0 + 155.0;
                    let g_base = (plasma * pi + 2.0).sin() * 30.0 + 40.0;
                    let b_base = (plasma * pi + 4.0).sin() * 55.0 + 200.0;

                    let gray = (r_base + g_base + b_base) / 3.0;
                    let dim_f64 = logo_dim as f64;
                    let r = ((r_base * ramp_progress as f64 + gray * (1.0 - ramp_progress as f64))
                        * dim_f64) as u8;
                    let g = ((g_base * ramp_progress as f64 + gray * (1.0 - ramp_progress as f64))
                        * dim_f64) as u8;
                    let b = ((b_base * ramp_progress as f64 + gray * (1.0 - ramp_progress as f64))
                        * dim_f64) as u8;

                    let style = Style::default()
                        .fg(Color::Rgb(r, g, b))
                        .add_modifier(Modifier::BOLD);

                    let target_y = center_y_start + row_idx + drip_offset;
                    if target_y < area.height {
                        // Check transparency for text
                        if (ch == ' ' || ch == '\u{2800}') {
                            // FIXME: Invert condition
                        } else {
                            buf.get_mut(x, target_y).set_char(ch).set_style(style);
                        }
                    }
                    x += 1;
                }
                row_idx += 1;
            }
        }

        // 4. Draw Prompt
        if phantom_dim > 0.1 && app.tick_count % 60 < 30 {
            let prompt_text = "Press any key to continue...";
            let prompt_x = (area.width.saturating_sub(prompt_text.len() as u16)) / 2;
            let prompt_y = area.height.saturating_sub(2);
            if prompt_y < area.height {
                let style = Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::DIM);
                buf.set_string(prompt_x, prompt_y, prompt_text, style);
            }
        }

        // 5. Subtle Credits
        if phantom_dim > 0.1 {
            let is_typo = (app.tick_count / 20) % 10 == 0;
            let credits_text = if is_typo {
                "prodact by @cafkafk"
            } else {
                "product by @cafkafk"
            };
            let credits_len = credits_text.len() as u16;
            let credits_x = area.width.saturating_sub(credits_len + 2);
            let credits_y = area.height.saturating_sub(2);

            if credits_x < area.width && credits_y < area.height {
                let style = Style::default().fg(Color::Rgb(180, 180, 190));
                buf.set_string(credits_x, credits_y, credits_text, style);
            }
        }

        // 6. Wired Phantoms (Cycle 5+) - Overlay
        if cycle_idx >= 5 {
            for phantom in &app.wired_state.phantoms {
                let elapsed = app.tick_count.saturating_sub(phantom.spawn_tick);
                if elapsed >= phantom.lifetime {
                    continue;
                }

                let progress = elapsed as f32 / phantom.lifetime as f32;

                let opacity = if progress < 0.2 {
                    progress / 0.2
                } else if progress > 0.8 {
                    1.0 - ((progress - 0.8) / 0.2)
                } else {
                    1.0
                };

                let gray_val = (255.0 * opacity * phantom_dim) as u8;
                if gray_val < 10 {
                    continue;
                }

                let text = LAIN_QUOTES[phantom.text_idx % LAIN_QUOTES.len()];
                let style = Style::default().fg(Color::Rgb(gray_val, gray_val, gray_val));

                if phantom.x < area.width && phantom.y < area.height {
                    buf.set_string(phantom.x, phantom.y, text, style);
                }
            }
        }

        // 7. Exit Animation (Cycle 5+ End)
        if cycle_idx >= 5 {
            let start_tick = 5 * 700;
            let elapsed = app.tick_count.saturating_sub(start_tick);
            let anim_start = 1200 + 600; // 1800
            if elapsed >= anim_start {
                draw_exit_sequence(area, buf, elapsed - anim_start);
            }
        }
    }
}

fn draw_exit_sequence(area: Rect, buf: &mut Buffer, t: u64) {
    let start_x = area.width.saturating_sub(30) / 2;
    let line1_y = (area.height / 2).saturating_sub(1);
    let line2_y = area.height / 2;

    if t < 454 {
        if line1_y < area.height && line2_y < area.height {
            // Line 1: "Close the world..."
            if t > 0 {
                let s_close = "Close";
                if t >= 0 {
                    buf.set_string(start_x, line1_y, s_close, Style::default().fg(Color::White));
                }
                if t >= 9 {
                    buf.set_string(
                        start_x + 5,
                        line1_y,
                        " the",
                        Style::default().fg(Color::White),
                    );
                }
                if t >= 18 {
                    buf.set_string(
                        start_x + 9,
                        line1_y,
                        " world...",
                        Style::default().fg(Color::White),
                    );
                }
            }

            // Line 2: "          ...Open the Next" -> Backspace -> "          ...txEn eht nepO"
            if t >= 110 {
                let base = "          ...";
                let target = "txEn eht nepO";
                let mut current_t = 110;
                let mut visible_chars = 0;

                for ch in target.chars() {
                    if t >= current_t {
                        visible_chars += 1;
                    }
                    // Typing speed matching simulation
                    if ch == ' ' {
                        current_t += 9;
                    } else {
                        current_t += 1;
                    }
                }

                let chars_to_show: String = target.chars().take(visible_chars).collect();
                let full = format!("{}{}", base, chars_to_show);
                buf.set_string(start_x, line2_y, &full, Style::default().fg(Color::White));

                if t >= 154 {
                    if (t / 30) % 2 == 0 {
                        let cursor_x = start_x + full.len() as u16;
                        buf.set_string(cursor_x, line2_y, "█", Style::default().fg(Color::White));
                    }
                }
            } else if t >= 84 {
                // Backspacing "          ...Open the Next"
                // Delete from end.
                let full = "          ...Open the Next";
                let delete_count = ((t - 84) / 2).min(13) as usize;
                let visible_len = full.len().saturating_sub(delete_count);
                let visible = &full[0..visible_len];
                buf.set_string(start_x, line2_y, visible, Style::default().fg(Color::White));
            } else if t >= 36 {
                // Typing "          ...Open the Next"
                let prefix = "          ";
                buf.set_string(start_x, line2_y, prefix, Style::default());
                if t >= 36 {
                    buf.set_string(
                        start_x + 10,
                        line2_y,
                        "...Open",
                        Style::default().fg(Color::White),
                    );
                }
                if t >= 45 {
                    buf.set_string(
                        start_x + 10 + 7,
                        line2_y,
                        " the",
                        Style::default().fg(Color::White),
                    );
                }
                if t >= 54 {
                    buf.set_string(
                        start_x + 10 + 7 + 4,
                        line2_y,
                        " Next",
                        Style::default().fg(Color::White),
                    );
                }
            }
        }
    }
    // If t >= 454, we draw nothing (Black screen)
}
