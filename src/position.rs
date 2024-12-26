use std::{io::stdout, str::FromStr, sync::Arc, time::Duration};

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
    db::{Database, PositionStatus},
    instructions::{
        claim_fee::claim_fee,
        close_position::close_position,
        remove_liquidity::remove_all_liquidity,
        swap_exact_in::{self, SwapExactInParameters},
    },
    pair::{self, get_pair},
    parser::parse_position_data,
    position_process::process_positions,
    token,
};

pub struct PositionManager<'a> {
    program: &'a Arc<Program<Arc<Keypair>>>,
    wallet: Pubkey,
    pub scroll_offset: usize,
    db: Database,
}

impl<'a> PositionManager<'a> {
    pub fn new(program: &'a Arc<Program<Arc<Keypair>>>, wallet: Pubkey, db: Database) -> Self {
        Self {
            program,
            wallet,
            scroll_offset: 0,
            db,
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
        _tx: &mpsc::Sender<UiMessage>,
    ) -> Result<()> {
        if position_info.active_bin_price < position_info.lower_bin_price {
            let insert_result = self
                .db
                .position
                .insert_stop_loss(
                    &position_info.pubkey,
                    &position_info.lb_pair,
                    &position_info.token_x_mint,
                    &position_info.token_y_mint,
                )
                .await?;
            if insert_result {
                info!("stop loss triggered: {}", position_info.pubkey);
            }

            // tx.send(UiMessage::Position(PositionMessage::Close(
            //     position_info.pubkey,
            // )))
            // .await?;
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
        let db = self.db.clone();
        let update_handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            let manager = PositionManager::new(&program_clone, wallet, db);

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

        let program_clone = self.program.clone();
        let update_tx_clone = tx.clone();
        let db_clone = self.db.clone();

        let position_process_handler = tokio::spawn(async move {
            process_positions(
                &program_clone,
                db_clone,
                Duration::from_secs(60),
                &update_tx_clone,
            )
            .await
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
                UiMessage::Position(PositionMessage::RemoveLiquidity(position, lb_pair)) => {
                    if let Err(err) = async {
                        info!(
                            "Processing remove liquidity position request for {}",
                            position
                        );
                        let commitment_config = CommitmentConfig::confirmed();
                        let transaction_config = RpcSendTransactionConfig {
                            skip_preflight: false,
                            preflight_commitment: Some(commitment_config.commitment),
                            encoding: None,
                            max_retries: None,
                            min_context_slot: None,
                        };

                        remove_all_liquidity(lb_pair, position, self.program, transaction_config)
                            .await?;

                        if let Err(err) = self
                            .db
                            .position
                            .update_status(&position.to_string(), PositionStatus::WaitClaimFees)
                            .await
                        {
                            error!(
                                "Failed to update status: {}: {:?}, err: {}",
                                position,
                                PositionStatus::WaitClaimFees,
                                err
                            )
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        error!("Failed to close position {}: {}", position, err);
                    }
                }
                UiMessage::Position(PositionMessage::ClaimFees(position)) => {
                    if let Err(err) = async {
                        info!("Processing claim fee position request for {}", position);
                        let commitment_config = CommitmentConfig::confirmed();
                        let transaction_config = RpcSendTransactionConfig {
                            skip_preflight: false,
                            preflight_commitment: Some(commitment_config.commitment),
                            encoding: None,
                            max_retries: None,
                            min_context_slot: None,
                        };

                        claim_fee(position, self.program, transaction_config).await?;

                        if let Err(err) = self
                            .db
                            .position
                            .update_status(&position.to_string(), PositionStatus::WaitClose)
                            .await
                        {
                            error!(
                                "Failed to update status: {}: {:?}, err: {}",
                                position,
                                PositionStatus::WaitClose,
                                err
                            )
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        error!("Failed to close position {}: {}", position, err);
                    }
                }
                UiMessage::Position(PositionMessage::Close(position)) => {
                    if let Err(err) = async {
                        info!("Processing close position request for {}", position);
                        let commitment_config = CommitmentConfig::confirmed();
                        let transaction_config = RpcSendTransactionConfig {
                            skip_preflight: false,
                            preflight_commitment: Some(commitment_config.commitment),
                            encoding: None,
                            max_retries: None,
                            min_context_slot: None,
                        };

                        close_position(position, self.program, transaction_config).await?;

                        if let Err(err) = self
                            .db
                            .position
                            .update_status(&position.to_string(), PositionStatus::WaitSellAll)
                            .await
                        {
                            error!(
                                "Failed to update status: {}: {:?}, err: {}",
                                position,
                                PositionStatus::WaitSellAll,
                                err
                            )
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        error!("Failed to close position {}: {}", position, err);
                    }
                }
                UiMessage::Position(PositionMessage::SellAll(position)) => {
                    if let Err(err) = async {
                        info!("Processing sell all position request for {}", position);
                        let position_data = self.db.position.get(&position).await?;

                        let commitment_config = CommitmentConfig::confirmed();
                        let transaction_config = RpcSendTransactionConfig {
                            skip_preflight: false,
                            preflight_commitment: Some(commitment_config.commitment),
                            encoding: None,
                            max_retries: None,
                            min_context_slot: None,
                        };
                        // query token account
                        let x_account = token::token_account(
                            &self.program.async_rpc(),
                            &self.wallet,
                            Pubkey::from_str(&position_data.token_x_mint)?,
                        )
                        .await?;

                        let amount_in = u64::from_str(&x_account.amount)?;
                        let params = SwapExactInParameters {
                            amount_in,
                            lb_pair: Pubkey::from_str(&position_data.lb_pair)?,
                            swap_for_y: true,
                        };
                        info!("swap: {:#?}", params);

                        swap_exact_in::swap(params, self.program, transaction_config).await?;

                        if let Err(err) = self
                            .db
                            .position
                            .update_status(&position.to_string(), PositionStatus::Terminate)
                            .await
                        {
                            error!(
                                "Failed to update status: {}: {:?}, err: {}",
                                position,
                                PositionStatus::Terminate,
                                err
                            )
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        error!("Failed to sell all position {}: {}", position, err);
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
        position_process_handler.abort();

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
    ClaimFees(Pubkey),
    RemoveLiquidity(Pubkey, Pubkey),
    Close(Pubkey),
    SellAll(Pubkey),
}

pub enum UiMessage {
    Quit,
    ScrollUp,
    ScrollDown,
    UpdatePositions(Vec<PositionInfo>),
    Position(PositionMessage),
}
