use anchor_lang::prelude::Pubkey;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, Pool, Sqlite, SqlitePool};

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct Position {
    pub id: i64,
    pub pubkey: String,
    pub lb_pair: String,
    pub token_x_mint: String,
    pub token_y_mint: String,
    pub status: String,
    pub created: i64,
    pub updated: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum PositionStatus {
    // wait remove liquidity
    WaitRemoveLiquidity,
    // wait claim fees
    WaitClaimFees,
    // wait close position
    WaitClose,
    // wait sell all position
    WaitSellAll,
    // terminate position
    Terminate,
}

impl ToString for PositionStatus {
    fn to_string(&self) -> String {
        match self {
            PositionStatus::WaitRemoveLiquidity => "wait_remove_liquidity".to_string(),
            PositionStatus::WaitClaimFees => "wait_claim_fees".to_string(),
            PositionStatus::WaitClose => "wait_close".to_string(),
            PositionStatus::WaitSellAll => "wait_sell_all".to_string(),
            PositionStatus::Terminate => "terminate".to_string(),
        }
    }
}

impl From<String> for PositionStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "wait_remove_liquidity" => PositionStatus::WaitRemoveLiquidity,
            "wait_claim_fees" => PositionStatus::WaitClaimFees,
            "wait_close" => PositionStatus::WaitClose,
            "wait_sell_all" => PositionStatus::WaitSellAll,
            "terminate" => PositionStatus::Terminate,
            _ => panic!("Invalid position status"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Database {
    pub pool: Pool<Sqlite>,
    pub position: PositionsModel,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Self {
            pool: pool.clone(),
            position: PositionsModel::new(pool),
        })
    }
}
#[derive(Debug, Clone)]
pub struct PositionsModel {
    pool: Pool<Sqlite>,
}
impl PositionsModel {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn get(&self, pubkey: &Pubkey) -> Result<Position> {
        let position = sqlx::query_as::<_, Position>(
            r#"
            SELECT * FROM positions
            WHERE pubkey = ? and status <> 'terminate'
            "#,
        )
        .bind(pubkey.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(position)
    }

    pub async fn has_active(&self, pubkey: &Pubkey) -> Result<bool> {
        let row = sqlx::query_as::<_, Position>(
            r#"
            SELECT * FROM positions
            WHERE pubkey = ? and status <> 'terminate'
            "#,
        )
        .bind(pubkey.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn insert_stop_loss(
        &self,
        pubkey: &Pubkey,
        lb_pair: &Pubkey,
        token_x_mint: &Pubkey,
        token_y_mint: &Pubkey,
    ) -> Result<bool> {
        if self.has_active(pubkey).await? {
            return Ok(false);
        }

        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO positions
            (pubkey, lb_pair, token_x_mint, token_y_mint, status, created, updated)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(pubkey.to_string())
        .bind(lb_pair.to_string())
        .bind(token_x_mint.to_string())
        .bind(token_y_mint.to_string())
        .bind(PositionStatus::WaitRemoveLiquidity.to_string())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    pub async fn get_with_status(&self, status: PositionStatus) -> Result<Vec<Position>> {
        let positions = sqlx::query_as(
            r#"
            SELECT * FROM positions
            WHERE status = ?
            "#,
        )
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(positions)
    }

    pub async fn update_status(&self, position: &str, status: PositionStatus) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            UPDATE positions
            SET status = ?, updated = ?
            WHERE pubkey = ?
            "#,
        )
        .bind(status.to_string())
        .bind(now)
        .bind(position)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
