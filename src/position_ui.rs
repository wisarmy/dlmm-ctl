use crate::position::{PositionInfo, PositionManager};
use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

impl<'a> PositionManager<'a> {
    // 添加绘制界面的方法
    pub fn draw_ui(
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        position_infos: &[PositionInfo],
    ) -> Result<()> {
        terminal.draw(|f| {
            let size = f.area();

            // 创建垂直布局
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3), // 标题
                        Constraint::Min(0),    // position 列表
                    ]
                    .as_ref(),
                )
                .split(size);

            // 绘制标题
            let title = Paragraph::new("Position Monitor")
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(title, chunks[0]);

            // 为每个 position 创建显示区域
            let position_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    position_infos
                        .iter()
                        .map(|_| Constraint::Length(6))
                        .collect::<Vec<_>>(),
                )
                .split(chunks[1]);

            // 绘制每个 position 的信息
            for (i, position) in position_infos.iter().enumerate() {
                let pos_block = Self::render_position_block(position);
                f.render_widget(pos_block, position_chunks[i]);
            }

            // 添加帮助信息
            let help_text = Line::from(vec![
                Span::raw("Press "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::raw(" to quit"),
            ]);

            let help = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));

            // 在底部显示帮助信息
            let help_chunk = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(f.area())[1];

            f.render_widget(help, help_chunk);
        })?;

        Ok(())
    }

    // 渲染单个 position 信息块
    fn render_position_block(info: &PositionInfo) -> Paragraph {
        let price_bar = Self::create_price_bar(info);

        let content = vec![
            Line::from(vec![
                Span::raw(format!("Position: {} ", info.pubkey)),
                Span::raw(format!("Pair: {}", info.lb_pair)),
            ]),
            Line::from(vec![Span::raw(format!(
                "Price Range: {} - {}",
                info.lower_bin_price, info.upper_bin_price
            ))]),
            Line::from(price_bar),
            Line::from(vec![Span::raw(format!(
                "Current Price: {}",
                info.active_bin_price
            ))]),
        ];

        Paragraph::new(content).block(Block::default().borders(Borders::ALL))
    }

    // 创建价格条
    fn create_price_bar(info: &PositionInfo) -> Vec<Span> {
        let mut bar = vec![];

        // 获取区间总长度
        let bin_range = info.upper_bin_id - info.lower_bin_id + 1;

        // bar的宽度使用实际的bin数量
        let bar_width = bin_range as usize;

        // 计算当前价格在区间内的相对位置
        let position = (info.active_bin_id - info.lower_bin_id) as usize;

        for i in 0..bar_width {
            let span = if i < position {
                // 当前价格以下的区域 - 蓝色
                Span::styled("▐", Style::default().fg(Color::Blue))
            } else if i == position {
                // 当前价格位置 - 黑色
                Span::styled("▐", Style::default().fg(Color::Yellow))
            } else {
                // 当前价格以上的区域 - 紫色
                Span::styled("▐", Style::default().fg(Color::Magenta))
            };
            bar.push(span);
        }

        bar
    }

    pub fn draw_loading(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        terminal.draw(|f| {
            let loading = Paragraph::new("Loading positions...")
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
