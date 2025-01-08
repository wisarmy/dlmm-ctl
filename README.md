# dlmm-ctl

1. Install sqlx-cli tool
```shell
cargo install sqlx-cli
```

2. Configuration
```shell
cp .env.example .env
```

3. Database setup
```shell
mkdir data
sqlx database create
# If using binary release, please manually copy migrations dir before running this command
sqlx migrate run
```

4. Build
```shell
cargo build -r
```

5. Run
```shell
# Monitor positions
./target/release/dlmm-ctl monitor-positions

# Monitor positions with stop-loss enabled
./target/release/dlmm-ctl monitor-positions --stop-loss
```
