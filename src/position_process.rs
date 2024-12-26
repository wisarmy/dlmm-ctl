use std::{str::FromStr, sync::Arc, time::Duration};

use anchor_client::{solana_sdk::signature::Keypair, Program};
use anchor_lang::prelude::Pubkey;
use anyhow::Result;
use tokio::sync::mpsc;

use crate::{
    db::{Database, PositionStatus},
    position::{PositionMessage, UiMessage},
};
use tracing::info;

pub async fn process_positions(
    _program: &Arc<Program<Arc<Keypair>>>,
    db: Database,
    interval: Duration,
    tx: &mpsc::Sender<UiMessage>,
) -> Result<()> {
    let mut interval_timer = tokio::time::interval(interval);

    loop {
        interval_timer.tick().await;
        // remove liquidity
        let positions = db
            .position
            .get_with_status(PositionStatus::WaitRemoveLiquidity)
            .await?;
        for position in positions {
            info!("Found wait remove liquidity position: {}", position.pubkey);
            tx.send(UiMessage::Position(PositionMessage::RemoveLiquidity(
                Pubkey::from_str(&position.pubkey)?,
                Pubkey::from_str(&position.lb_pair)?,
            )))
            .await?;
        }

        // claim fees
        let positions = db
            .position
            .get_with_status(PositionStatus::WaitClaimFees)
            .await?;
        for position in positions {
            info!("Found wait claim fees position: {}", position.pubkey);
            tx.send(UiMessage::Position(PositionMessage::ClaimFees(
                Pubkey::from_str(&position.pubkey)?,
            )))
            .await?;
        }

        // close positions
        let positions = db
            .position
            .get_with_status(PositionStatus::WaitClose)
            .await?;
        for position in positions {
            info!("Found wait close position: {}", position.pubkey);
            tx.send(UiMessage::Position(PositionMessage::Close(
                Pubkey::from_str(&position.pubkey)?,
            )))
            .await?;
        }
        // sell all positions
        let positions = db
            .position
            .get_with_status(PositionStatus::WaitSellAll)
            .await?;
        for position in positions {
            info!("Found wait sell all position: {}", position.pubkey);
            tx.send(UiMessage::Position(PositionMessage::SellAll(
                Pubkey::from_str(&position.pubkey)?,
            )))
            .await?;
        }
    }
}
