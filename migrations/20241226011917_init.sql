CREATE TABLE IF NOT EXISTS positions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pubkey TEXT NOT NULL,
    lb_pair TEXT NOT NULL,
    token_x_mint TEXT NOT NULL,
    token_y_mint TEXT NOT NULL,
    status TEXT NOT NULL,
    created INTEGER NOT NULL DEFAULT 0,
    updated INTEGER NOT NULL Default 0
);

CREATE INDEX IF NOT EXISTS idx_positions_status ON positions (status);
