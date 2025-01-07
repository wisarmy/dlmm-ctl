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
use anchor_spl::token::spl_token;
use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use lb_clmm::state::position::PositionV2;
use ratatui::{prelude::CrosstermBackend, Terminal};
use solana_account_decoder::UiAccountEncoding;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{
    db::{Database, PositionStatus},
    instructions::{
        claim_fee::claim_fee,
        close_position::close_position,
        remove_liquidity::remove_all_liquidity,
        swap_exact_in::{self, SwapExactInParameters},
    },
    pair::{self, get_pair, get_pair_api_info},
    parser::parse_position_data,
    position::{PositionInfo, PositionMessage, UiMessage},
    position_process::process_positions,
    token,
};

pub struct PositionManager<'a> {
    pub program: &'a Arc<Program<Arc<Keypair>>>,
    wallet: Pubkey,
    db: Database,
    pub scroll_offset: usize,
    pub status_message: String,
    pub current_positions: Vec<PositionInfo>,
}

impl<'a> PositionManager<'a> {
    pub fn new(program: &'a Arc<Program<Arc<Keypair>>>, wallet: Pubkey, db: Database) -> Self {
        Self {
            program,
            wallet,
            db,
            scroll_offset: 0,
            status_message: "".to_string(),
            current_positions: vec![],
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
        // get pair api info
        let pair_api_info = get_pair_api_info(&position.lb_pair.to_string()).await.ok();
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
            position_state: position,
            pair_info: pair_info.clone(),
            pubkey,
            lb_pair: position.lb_pair,
            pair_name: pair_api_info.map(|info| info.name),
            token_x_mint: pair_info.token_x_mint,
            token_y_mint: pair_info.token_y_mint,
            x_mint: pair_info.x_mint,
            y_mint: pair_info.y_mint,
            lower_bin_id: position.lower_bin_id,
            lower_bin_price,
            upper_bin_id: position.upper_bin_id,
            upper_bin_price,
            active_bin_id: pair_info.active_id,
            active_bin_price: pair_info.token_price,
            last_updated_at: position.last_updated_at,
            balance: None,
            token_x_price: None,
            token_y_price: None,
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
        }
        Ok(())
    }

    pub async fn monitor_positions(&mut self, interval: Duration, stop_loss: bool) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        Self::draw_loading(&mut terminal, "Loading positions...")?;

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
            let program = program_clone.clone();

            loop {
                interval_timer.tick().await;
                let _ = update_tx
                    .send(UiMessage::UpdateStatus(format!(
                        "Fetching positions for {}",
                        wallet
                    )))
                    .await;
                info!("Fetching positions for {}", wallet);
                match manager.get_positions().await {
                    Ok(raw_positions) => {
                        let mut position_infos = vec![];
                        for (pubkey, position) in raw_positions {
                            let _ = update_tx
                                .send(UiMessage::UpdateStatus(format!(
                                    "Fetching position: {}",
                                    pubkey
                                )))
                                .await;
                            debug!("Found position: {}, {:#?}", pubkey, position);
                            info!(
                                "Fetching position: {}, lb_pair: {}, last_updated: {}",
                                pubkey,
                                position.lb_pair,
                                DateTime::from_timestamp(position.last_updated_at, 0)
                                    .map(|utc| utc.with_timezone(&Local))
                                    .map(|local| local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
                                    .unwrap_or_else(|| "invalid timestamp".to_string())
                            );
                            if let Ok(mut position_info) = manager.get_info(pubkey, position).await
                            {
                                let _ = update_tx
                                    .send(UiMessage::UpdateStatus(format!(
                                        "Fetching position balance: {}",
                                        pubkey
                                    )))
                                    .await;
                                position_info.with_balance(&program).await.ok();
                                position_info.with_price().await.ok();
                                // 只处理token_y_mint为wsol的position
                                if position_info.token_y_mint == spl_token::native_mint::id() {
                                    if stop_loss {
                                        let _ = manager
                                            .stop_loss(position_info.clone(), &update_tx)
                                            .await;
                                    }
                                    position_infos.push(position_info);
                                } else {
                                    warn!(
                                        "Invalid position: {}, lb_pair: {}, token_y_mint is not {}",
                                        pubkey,
                                        position_info.lb_pair,
                                        spl_token::native_mint::id()
                                    );
                                }
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

        while let Some(msg) = rx.recv().await {
            match msg {
                UiMessage::Quit => break,
                UiMessage::ScrollUp => {
                    if let Err(err) = {
                        if self.scroll_offset > 0 {
                            self.scroll_offset -= 1;
                            self.draw_ui(&mut terminal).await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    } {
                        error!("Failed to update UI: {}", err);
                    }
                }
                UiMessage::ScrollDown => {
                    if let Err(err) = {
                        if self.scroll_offset < self.current_positions.len().saturating_sub(1) {
                            self.scroll_offset += 1;
                            self.draw_ui(&mut terminal).await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    } {
                        error!("Failed to update UI: {}", err);
                    }
                }
                UiMessage::UpdateStatus(msg) => {
                    if let Err(err) = {
                        if self.current_positions.is_empty() {
                            Self::draw_loading(&mut terminal, msg)?;
                        } else {
                            self.status_message = msg;
                            self.draw_ui(&mut terminal).await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    } {
                        error!("Failed to update UI: {}", err);
                    }
                }
                UiMessage::UpdatePositions(new_positions) => {
                    if let Err(err) = {
                        self.current_positions = new_positions;
                        self.status_message = format!("Last Updated: {}", Local::now());
                        self.draw_ui(&mut terminal).await?;
                        Ok::<(), anyhow::Error>(())
                    } {
                        error!("Failed to update UI: {}", err);
                    }
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
