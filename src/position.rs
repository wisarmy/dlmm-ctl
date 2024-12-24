use std::{io::stdout, rc::Rc, time::Duration};

use anchor_client::{
    anchor_lang::Space,
    solana_client::{
        rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
        rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    Program,
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use lb_clmm::state::position::PositionV2;
use ratatui::{prelude::CrosstermBackend, Terminal};
use rust_decimal::Decimal;
use solana_account_decoder::UiAccountEncoding;
use tokio::{select, sync::mpsc};
use tracing::{debug, error, info};

use crate::{
    pair::{self, get_pair},
    parser::parse_position_data,
    position_ui::UiMessage,
};

pub struct PositionManager<'a> {
    program: &'a Program<Rc<Keypair>>,
    wallet: Pubkey,
    pub scroll_offset: usize,
}

impl<'a> PositionManager<'a> {
    pub fn new(program: &'a Program<Rc<Keypair>>, wallet: Pubkey) -> Self {
        Self {
            program,
            wallet,
            scroll_offset: 0,
        }
    }

    pub async fn get_positions(&self) -> Result<Vec<(Pubkey, PositionV2)>> {
        let filters = vec![
            RpcFilterType::DataSize((8 + PositionV2::INIT_SPACE) as u64),
            RpcFilterType::Memcmp(Memcmp::new(
                0,
                MemcmpEncodedBytes::Base58("LgkNAEYaVX3".to_string()),
            )),
            RpcFilterType::Memcmp(Memcmp::new(
                40,
                MemcmpEncodedBytes::Base58(self.wallet.to_string()),
            )),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                ..Default::default()
            },
            ..Default::default()
        };

        let raw_accounts = self
            .program
            .async_rpc()
            .get_program_accounts_with_config(&lb_clmm::ID, config)
            .await?;

        let mut positions = Vec::new();
        for (pubkey, account) in raw_accounts {
            if let Ok(position) = parse_position_data(&account.data) {
                positions.push((pubkey, position));
            }
        }

        Ok(positions)
    }

    pub async fn get_info(&self, pubkey: Pubkey, position: PositionV2) -> Result<PositionInfo> {
        // get pair
        let pair_info = get_pair(position.lb_pair, self.program).await?;
        let lower_bin_price = pair::caculate_bin_price(
            pair_info.bin_step,
            position.lower_bin_id,
            &pair_info.x_mint,
            &pair_info.y_mint,
        )?;
        let upper_bin_price = pair::caculate_bin_price(
            pair_info.bin_step,
            position.upper_bin_id,
            &pair_info.x_mint,
            &pair_info.y_mint,
        )?;

        let position_info = PositionInfo {
            pubkey,
            lb_pair: position.lb_pair,
            lower_bin_id: position.lower_bin_id,
            lower_bin_price,
            upper_bin_id: position.upper_bin_id,
            upper_bin_price,
            active_bin_id: pair_info.active_id,
            active_bin_price: pair_info.token_price,
            last_updated_at: position.last_updated_at,
        };

        Ok(position_info)
    }
    /// stop loss strategy
    pub async fn stop_loss(&self, position_info: PositionInfo) -> Result<()> {
        if position_info.active_bin_price < position_info.lower_bin_price {
            info!("stop loss triggered, sell all");
            // TODO: sell all
        }
        Ok(())
    }

    pub async fn monitor_positions(&mut self, interval: Duration, stop_loss: bool) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        Self::draw_loading(&mut terminal)?;

        // 创建用于UI控制的channel
        let (tx, mut rx) = mpsc::channel(32);
        let input_tx = tx.clone();

        // 创建一个处理键盘输入的任务
        let input_handle = tokio::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        let msg = match key.code {
                            KeyCode::Char('q') => Some(UiMessage::Quit),
                            KeyCode::Up => Some(UiMessage::ScrollUp),
                            KeyCode::Down => Some(UiMessage::ScrollDown),
                            _ => None,
                        };
                        if let Some(msg) = msg {
                            if input_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });

        let mut positions = vec![];
        let mut interval_timer = tokio::time::interval(interval);

        // 封装重绘UI的函数
        let redraw = |terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
                      positions: &[PositionInfo],
                      scroll_offset: usize|
         -> Result<()> { Self::draw_ui(terminal, positions, scroll_offset) };

        loop {
            select! {
                // 处理UI消息
                biased;
                Some(msg) = rx.recv() => {
                    match msg {
                        UiMessage::Quit => break,
                        UiMessage::ScrollUp => {
                            if self.scroll_offset > 0 {
                                self.scroll_offset -= 1;
                                redraw(&mut terminal, &positions, self.scroll_offset)?;
                            }
                            info!("scroll up: {}", self.scroll_offset);
                        }
                        UiMessage::ScrollDown => {
                            if self.scroll_offset < positions.len() - 1 {
                                self.scroll_offset += 1;
                                redraw(&mut terminal, &positions, self.scroll_offset)?;
                            }

                            info!("scroll down: {}", self.scroll_offset);
                        }
                        UiMessage::UpdatePositions(new_positions) => {
                            info!("update positions in UI");
                            positions = new_positions;
                            redraw(&mut terminal, &positions, self.scroll_offset)?;
                        }
                    }
                }
                // 处理定时更新
                _ = interval_timer.tick() => {
                    match self.get_positions().await {
                        Ok(raw_positions) => {
                            let mut position_infos = vec![];
                            for (pubkey, position) in raw_positions {
                                debug!("position: {}, {:#?}", pubkey, position);
                                info!("fetch position: {}, lb_pair: {}", pubkey, position.lb_pair);
                                if let Ok(position_info) = self.get_info(pubkey, position).await {
                                    if stop_loss {
                                        let _ = self.stop_loss(position_info.clone()).await;
                                    }
                                    position_infos.push(position_info);
                                }
                            }
                            position_infos.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));
                            positions = position_infos;
                            redraw(&mut terminal, &positions, self.scroll_offset)?;
                        }
                        Err(e) => {
                            error!("Error updating positions: {}", e);
                        }
                    }
                }
            }
        }
        // 清理终端
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        // 等待输入处理任务结束
        let _ = input_handle.await;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PositionInfo {
    pub pubkey: Pubkey,
    pub lb_pair: Pubkey,
    pub lower_bin_id: i32,
    pub lower_bin_price: Decimal,
    pub upper_bin_id: i32,
    pub upper_bin_price: Decimal,
    pub active_bin_id: i32,
    pub active_bin_price: Decimal,
    pub last_updated_at: i64,
}
