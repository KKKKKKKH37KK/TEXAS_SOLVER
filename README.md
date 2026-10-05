# HEXAS Solver

6-max NLHE cash game GTO solver（瀏覽器 WASM + native CLI），只做賽後研究。規格見 [PRD.md](PRD.md)。

## 結構
```
crates/core/   cards, evaluator, tree, CFR（純 Rust）
crates/cli/    hexas 執行檔（測試與 benchmark）
web/           Vite + React + TS 前端
```

## 開發
```
cargo test --workspace           # Rust 測試
cargo test --release -p hexas-core --test evaluator -- --ignored   # 7 張牌窮舉驗證
cargo run --release -p hexas-cli -- --help
cargo run --release -p hexas-cli -- river --board Qs9h5d3c2s --oop "AA-22,AKs-A2s,KQo" --ip "TT-22,AQs-A2s,KQo-KJo"
cd web && npm install && npm run dev
```

Windows 上用 `stable-x86_64-pc-windows-gnu` toolchain（不需要 Visual Studio Build Tools）。
