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
cargo run -p hexas-cli -- cards AsKd7c
cd web && npm install && npm run dev
```

Windows 上用 `stable-x86_64-pc-windows-gnu` toolchain（不需要 Visual Studio Build Tools）。
