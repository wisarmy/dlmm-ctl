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
use tracing::debug;

use crate::{
    pair::{self, get_pair},
    parser::parse_position_data,
};

pub struct PositionManager<'a> {
    program: &'a Program<Rc<Keypair>>,
    wallet: Pubkey,
}

impl<'a> PositionManager<'a> {
    pub fn new(program: &'a Program<Rc<Keypair>>, wallet: Pubkey) -> Self {
        Self { program, wallet }
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

    pub async fn monitor_positions(&self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        Self::draw_loading(&mut terminal)?;

        // 创建用于发送退出信号的channel
        let (tx, mut rx) = mpsc::channel(1);
        // 创建一个处理键盘输入的任务
        let input_handle = tokio::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.code == KeyCode::Char('q') {
                            let _ = tx.send(()).await;
                            break;
                        }
                    }
                }
            }
        });
        loop {
            // 使用 select! 同时等待数据更新和退出信号
            select! {
                // 检查是否收到退出信号
                _ = rx.recv() => {
                    break;
                }
                _ = async {
                    let positions = self.get_positions().await?;
                    let mut position_infos = vec![];

                    for (pubkey, position) in positions {
                        debug!("position: {}, {:#?}", pubkey, position);
                        let position_info = self.get_info(pubkey, position).await?;
                        position_infos.push(position_info);
                    }
                    position_infos.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));
                    Self::draw_ui(&mut terminal, &position_infos)?;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok::<_, anyhow::Error>(())
                } => {}
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

#[derive(Debug)]
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
