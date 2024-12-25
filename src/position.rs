use std::{io::stdout, sync::Arc, time::Duration};

use anchor_client::{
    anchor_lang::Space,
    solana_client::{
        rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSendTransactionConfig},
        rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
    },
    solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair},
    Program,
};
use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use lb_clmm::state::position::PositionV2;
use ratatui::{prelude::CrosstermBackend, Terminal};
use rust_decimal::Decimal;
use solana_account_decoder::UiAccountEncoding;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::{
    instructions::close_position::close_position,
    pair::{self, get_pair},
    parser::parse_position_data,
};

pub struct PositionManager<'a> {
    program: &'a Arc<Program<Arc<Keypair>>>,
    wallet: Pubkey,
    pub scroll_offset: usize,
}

impl<'a> PositionManager<'a> {
    pub fn new(program: &'a Arc<Program<Arc<Keypair>>>, wallet: Pubkey) -> Self {
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
        debug!("{:#?}", pair_info);
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
            token_x_mint: pair_info.token_x_mint,
            token_y_mint: pair_info.token_y_mint,
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
    pub async fn stop_loss(
        &self,
        position_info: PositionInfo,
        tx: &mpsc::Sender<UiMessage>,
    ) -> Result<()> {
        if position_info.active_bin_price < position_info.lower_bin_price {
            info!("stop loss triggered: {}", position_info.pubkey);
            tx.send(UiMessage::Position(PositionMessage::Close(
                position_info.pubkey,
            )))
            .await?;
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
        let update_tx = tx.clone();

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

        // 创建数据更新任务
        let program_clone = self.program.clone();
        let wallet = self.wallet;
        let update_handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            let manager = PositionManager::new(&program_clone, wallet);

            loop {
                interval_timer.tick().await;
                info!("fetching positions for {}", wallet);
                match manager.get_positions().await {
                    Ok(raw_positions) => {
                        let mut position_infos = vec![];
                        for (pubkey, position) in raw_positions {
                            debug!("position: {}, {:#?}", pubkey, position);
                            info!(
                                "fetching info: position: {}, lb_pair: {}, last_updated: {}",
                                pubkey,
                                position.lb_pair,
                                DateTime::from_timestamp(position.last_updated_at, 0)
                                    .map(|utc| utc.with_timezone(&Local))
                                    .map(|local| local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
                                    .unwrap_or_else(|| "invalid timestamp".to_string())
                            );
                            if let Ok(position_info) = manager.get_info(pubkey, position).await {
                                if stop_loss {
                                    let _ =
                                        manager.stop_loss(position_info.clone(), &update_tx).await;
                                }
                                position_infos.push(position_info);
                            }
                        }
                        position_infos.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));

                        if update_tx
                            .send(UiMessage::UpdatePositions(position_infos))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error updating positions: {}", e);
                    }
                }
            }
        });

        let mut positions = vec![];

        // 封装重绘UI的函数
        let redraw = |terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
                      positions: &[PositionInfo],
                      scroll_offset: usize|
         -> Result<()> { Self::draw_ui(terminal, positions, scroll_offset) };

        while let Some(msg) = rx.recv().await {
            match msg {
                UiMessage::Quit => break,
                UiMessage::ScrollUp => {
                    if self.scroll_offset > 0 {
                        self.scroll_offset -= 1;
                        redraw(&mut terminal, &positions, self.scroll_offset)?;
                    }
                }
                UiMessage::ScrollDown => {
                    if self.scroll_offset < positions.len().saturating_sub(1) {
                        self.scroll_offset += 1;
                        redraw(&mut terminal, &positions, self.scroll_offset)?;
                    }
                }
                UiMessage::UpdatePositions(new_positions) => {
                    positions = new_positions;
                    redraw(&mut terminal, &positions, self.scroll_offset)?;
                }
                UiMessage::Position(PositionMessage::Close(position)) => {
                    info!("Processing close position request for {}", position);
                    let commitment_config = CommitmentConfig::confirmed();
                    let transaction_config = RpcSendTransactionConfig {
                        skip_preflight: false,
                        preflight_commitment: Some(commitment_config.commitment),
                        encoding: None,
                        max_retries: None,
                        min_context_slot: None,
                    };

                    if let Err(e) = close_position(position, self.program, transaction_config).await
                    {
                        error!("Failed to close position {}: {}", position, e);
                    }
                }
            }
        }

        // 清理终端
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        // 取消所有任务
        input_handle.abort();
        update_handle.abort();

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PositionInfo {
    pub pubkey: Pubkey,
    pub lb_pair: Pubkey,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub lower_bin_id: i32,
    pub lower_bin_price: Decimal,
    pub upper_bin_id: i32,
    pub upper_bin_price: Decimal,
    pub active_bin_id: i32,
    pub active_bin_price: Decimal,
    pub last_updated_at: i64,
}

#[derive(Debug)]
pub enum PositionMessage {
    Close(Pubkey),
}

pub enum UiMessage {
    Quit,
    ScrollUp,
    ScrollDown,
    UpdatePositions(Vec<PositionInfo>),
    Position(PositionMessage),
}
