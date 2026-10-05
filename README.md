# HEXAS Solver

6-max NLHE cash game GTO solver（瀏覽器 WASM + native CLI），只做賽後研究，不做牌桌即時輔助。規格見 [PRD.md](PRD.md)。

## 結構
```
crates/core/   cards, evaluator, game tree, DCFR, isomorphism, query, result files（純 Rust）
crates/cli/    hexas 執行檔：tree / solve / eval
crates/wasm/   瀏覽器介面：JSON 指令 + C ABI（不用 wasm-bindgen）
web/           Vite + React + TS 前端，solver 在 Web Worker 裡跑
```

## 怎麼用

**轉牌、河牌**：直接在網頁求解。
```
cd web
npm install
npm run dev        # 會先編 WASM，再開 http://localhost:5173
```

**翻牌**：樹通常有數 GB，用本機 CLI 解完再到網頁開啟。
```
cargo build --release
.\target\release\hexas.exe tree  --board Ks7s2d --oop "<BB 範圍>" --ip "<BTN 範圍>" --pot 5.5 --stack 97.5
.\target\release\hexas.exe solve --board Ks7s2d --oop "<BB 範圍>" --ip "<BTN 範圍>" --pot 5.5 --stack 97.5 `
    --budget-mb 12000 --target 0.5 --out results\ks7s2d.hxs
```
- `tree` 只估算記憶體，不求解。
- `--out` 寫出的結果檔存了翻牌和轉牌的策略。到網頁按「開啟結果檔」載入；走到河牌時，可以用那個節點的範圍在瀏覽器重解河牌。
- 樹的預設值依 PRD §3.3：
  - 翻牌 spot：翻牌 33/66/100/125%、不 donk；轉牌和河牌 66/125%；每條街最多加注 1 次。
  - 轉牌、河牌 spot：每條街 33/66/100/125%、最多加注 3 次。
  - 加注一律是 3× 對方下注，另外加 all-in。可以用 `--bets`、`--turn-bets`、`--river-bets`、`--max-raises`、`--donk` 覆寫。
- `hexas --help` 列出全部參數。

**翻前**：網頁上的「翻前 Preflop」分頁可以直接求解，約 5 秒。點到某條線的「看翻牌」終點，按「帶入翻後求解」，雙方範圍和底池會自動填進翻後頁面。CLI 用 `hexas preflop --iters 300`。翻前結果是近似解，限制見 PRD §4.2 和 §9.1。

**解庫（連續瀏覽）**：三條線（BTN vs BB SRP、BB 3bet BTN、BTN 3bet CO）事先解好全部 1,755 個代表翻牌。
```
.\target\release\hexas.exe library --line srp-btn-bb --hours 12            # 中斷後重跑會接續
.\target\release\hexas.exe library --line 3bp-bb-btn --hours 12 --threads 8
```
- 結果寫在 `library/<線>/`（不進 git），進度看 `index.csv`。
- `npm run dev` 時，網頁會從 `library/` 讀檔。在翻前頁面點到這三條線的「看翻牌」終點，輸入任意翻牌就能看到翻牌策略；轉牌和河牌用當下的範圍在瀏覽器重解（轉牌約 10 秒）。
- realization 表由 `hexas calibrate` 產生後寫死在程式碼裡；改了以後，解庫要整條重跑。

## 開發
```
cargo test --workspace                                            # Rust 測試
cargo test --release -p hexas-core --test evaluator -- --ignored  # 7 張牌窮舉驗證（約 1 秒）
cargo clippy --workspace --all-targets -- -D warnings
cd web && npm test && npm run build
```

- Windows 上用 `stable-x86_64-pc-windows-gnu` toolchain（不需要 Visual Studio Build Tools）。
- 和 postflop-solver 的對照程式（PRD §8.4）放在 repo 外，因為對方是 AGPL。
