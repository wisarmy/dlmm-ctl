use crate::{position::PositionInfo, position_manager::PositionManager};
use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use rust_decimal::prelude::ToPrimitive;

impl<'a> PositionManager<'a> {
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_down(&mut self, position_count: usize, visible_positions: usize) {
        if self.scroll_offset + visible_positions < position_count {
            self.scroll_offset += 1;
        }
    }

    // Draw the UI
    pub async fn draw_ui(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        terminal.draw(|f| {
            let size = f.area();

            // Calculate how many positions can be displayed in the visible area
            let visible_height = size.height as usize - 7; // the height of the title and help area
            let positions_per_page = visible_height / 8; //  the height of each position block

            // Get the visible positions
            let visible_positions = self
                .current_positions
                .iter()
                .skip(self.scroll_offset)
                .take(positions_per_page);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3), // title
                        Constraint::Min(0),    // positions
                        Constraint::Length(3), // help
                    ]
                    .as_ref(),
                )
                .split(size);

            // split the title area
            let title_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50), // left title
                    Constraint::Percentage(50), // right status
                ])
                .split(chunks[0]);

            let (total_value, total_fee): (f64, f64) =
                self.current_positions
                    .iter()
                    .fold((0.0, 0.0), |(value_acc, fee_acc), position| {
                        let position_value = if let Some((amount_x, amount_y, fee_x, fee_y)) =
                            position.balance
                        {
                            // calculate the total value of the current position
                            let value =
                                position.token_x_price.map_or(0.0, |price| amount_x * price)
                                    + position.token_y_price.map_or(0.0, |price| amount_y * price);

                            // calculate the total fee of the current position
                            let fee = position.token_x_price.map_or(0.0, |price| fee_x * price)
                                + position.token_y_price.map_or(0.0, |price| fee_y * price);

                            (value, fee)
                        } else {
                            (0.0, 0.0)
                        };

                        (value_acc + position_value.0, fee_acc + position_value.1)
                    });

            // left title
            let title_text = Line::from(vec![
                Span::raw(format!(
                    "Position Monitor ({}/{} positions) ",
                    (self.scroll_offset + 1).min(self.current_positions.len()),
                    self.current_positions.len()
                )),
                Span::styled(
                    format!("${:.2}", total_value),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(" + "),
                Span::styled(
                    format!("${:.2}", total_fee),
                    Style::default().fg(Color::Magenta),
                ),
                Span::raw(" = "),
                Span::styled(
                    format!("${:.2}", total_value + total_fee),
                    Style::default().fg(Color::Cyan),
                ),
            ]);
            let title = Paragraph::new(title_text)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::LEFT | Borders::BOTTOM | Borders::TOP));

            // right status
            let status_message = Paragraph::new(format!("{}", self.status_message))
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Right)
                .block(Block::default().borders(Borders::RIGHT | Borders::BOTTOM | Borders::TOP));

            f.render_widget(title, title_chunks[0]);
            f.render_widget(status_message, title_chunks[1]);

            // Create a layout with vertical direction
            let position_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    visible_positions
                        .clone()
                        .map(|_| Constraint::Length(8))
                        .collect::<Vec<_>>(),
                )
                .split(chunks[1]);

            // render each position block
            for (i, info) in visible_positions.enumerate() {
                self.render_position_block(f, position_chunks[i], info);
                // f.render_widget(pos_block, position_chunks[i]);
            }

            // split the help area into left and right parts
            let help_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(70), // left help
                    Constraint::Percentage(30), // right sol price
                ])
                .split(chunks[2]);

            // help message
            let help_text = Line::from(vec![
                Span::raw("Press "),
                Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
                Span::raw(" to scroll, "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(" to quit"),
            ]);
            // sol price
            let sol_text = Line::from(vec![
                Span::raw("SOL: "),
                Span::styled(
                    format!("${:.2}", self.sol_price),
                    Style::default().fg(Color::Green),
                ),
            ]);
            f.render_widget(
                Paragraph::new(help_text).block(
                    Block::default().borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM),
                ),
                help_chunks[0],
            );
            f.render_widget(
                Paragraph::new(sol_text).alignment(Alignment::Right).block(
                    Block::default().borders(Borders::TOP | Borders::RIGHT | Borders::BOTTOM),
                ),
                help_chunks[1],
            );
        })?;

        Ok(())
    }

    // Render the position block
    fn render_position_block<'b>(
        &self,
        f: &mut Frame,
        area: ratatui::prelude::Rect,
        position_info: &'b PositionInfo,
    ) {
        // Split the area into two parts: top and bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // top part: basic info
                Constraint::Min(3),    // bottom part: balance info
            ])
            .split(area);

        // Split the bottom part into two parts: left and right
        let balance_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // left：Current Balance
                Constraint::Percentage(50), // right：Unclaimed Swap Fee
            ])
            .split(main_chunks[1]);

        let pair_names = position_info
            .get_pair_names()
            .unwrap_or(("X".to_string(), "Y".to_string()));

        // draw basic info
        let basic_info = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!(
                        "{} ",
                        position_info.pair_name.as_deref().unwrap_or_default()
                    ),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(format!("Position: {} ", position_info.pubkey)),
                Span::raw(format!("Pair: {}", position_info.lb_pair)),
            ]),
            Line::from(vec![Span::raw(format!(
                "Price Range: {} - {}",
                position_info.lower_bin_price, position_info.upper_bin_price
            ))]),
            Line::from(Self::create_price_bar(position_info)),
            Line::from(vec![
                Span::raw(format!("Current Price: {}", position_info.active_bin_price)),
                Span::styled(
                    format!(
                        " (${})",
                        position_info.active_bin_price.to_f64().unwrap_or(0.0) * self.sol_price
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT));
        f.render_widget(basic_info, main_chunks[0]);

        if let Some((amount_x, amount_y, fee_x, fee_y)) = position_info.balance {
            let total_balance = position_info
                .token_x_price
                .map_or(0.0, |price| amount_x * price)
                + position_info
                    .token_y_price
                    .map_or(0.0, |price| amount_y * price);
            let total_fee = position_info
                .token_x_price
                .map_or(0.0, |price| fee_x * price)
                + position_info
                    .token_y_price
                    .map_or(0.0, |price| fee_y * price);
            // draw Current Balance
            let current_balance = Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    format!("{}: {:.6} ", pair_names.0, amount_x,),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!(
                        "(${:.2})",
                        position_info
                            .token_x_price
                            .map_or(0.0, |price| amount_x * price)
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("    {}: {:.6} ", pair_names.1, amount_y),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!(
                        "(${:.2})",
                        position_info
                            .token_y_price
                            .map_or(0.0, |price| amount_y * price)
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ])])
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::BOTTOM)
                    .title(Line::from(vec![
                        Span::raw("Current Balance: "),
                        Span::styled(
                            format!("${:.2}", total_balance),
                            Style::default().fg(Color::Blue),
                        ),
                    ])),
            );
            f.render_widget(current_balance, balance_chunks[0]);

            // draw Unclaimed Swap Fee
            let unclaimed_fee = Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    format!("{}: {:.6} ", pair_names.0, fee_x),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!(
                        "(${:.2})",
                        position_info
                            .token_x_price
                            .map_or(0.0, |price| fee_x * price)
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("    {}: {:.6} ", pair_names.1, fee_y,),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!(
                        "(${:.2})",
                        position_info
                            .token_y_price
                            .map_or(0.0, |price| fee_y * price)
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ])])
            .block(
                Block::default()
                    .borders(Borders::RIGHT | Borders::BOTTOM)
                    .title(Line::from(vec![
                        Span::raw("Unclaimed Swap Fee: "),
                        Span::styled(
                            format!("${:.2}", total_fee),
                            Style::default().fg(Color::Magenta),
                        ),
                    ])),
            );
            f.render_widget(unclaimed_fee, balance_chunks[1]);
        } else {
            // draw error message
            let error_msg = Paragraph::new("Failed to get balance")
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM));
            f.render_widget(error_msg, main_chunks[1]);
        }
    }

    // Create a price bar
    fn create_price_bar(info: &PositionInfo) -> Vec<Span> {
        let mut bar = vec![];

        let bin_range = info.upper_bin_id - info.lower_bin_id + 1;
        let bar_width = bin_range as usize;

        // Calculate the position of the current price in the bar
        let position = (info.active_bin_id - info.lower_bin_id) as usize;

        for i in 0..bar_width {
            let span = if i < position {
                // 当前价格以下的区域
                Span::styled("▐", Style::default().fg(Color::Green))
            } else if i == position {
                // 当前价格位置
                Span::styled("▐", Style::default().fg(Color::Yellow))
            } else {
                // 当前价格以上的区域
                Span::styled("▐", Style::default().fg(Color::Red))
            };
            bar.push(span);
        }

        bar
    }

    pub fn draw_loading(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        msg: impl Into<String>,
    ) -> Result<()> {
        terminal.draw(|f| {
            let loading = Paragraph::new(msg.into())
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL));

            let area = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Percentage(40),
                        Constraint::Length(3),
                        Constraint::Percentage(40),
                    ]
                    .as_ref(),
                )
                .split(f.area())[1];

            f.render_widget(loading, area);
        })?;

        Ok(())
    }
}
