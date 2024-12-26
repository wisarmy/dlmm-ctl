# dlmm-ctl

1. 安装 sqlx-cli 工具
```shell
cargo install sqlx-cli
```

2. 配置
```shell
cp .env.example .env
```

3. 数据库配置
```shell
mkdir data
sqlx database create
sqlx migrate run
```

4. 编译
```shell
cargo build -r
```

5. 运行
```shell
# 监控仓位
./target/release/dlmm-ctl monitor-positions

# 监控仓位并设置止损
./target/release/dlmm-ctl monitor-positions --stop-loss
```
