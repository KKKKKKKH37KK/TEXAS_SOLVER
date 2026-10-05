# PRD：HEXAS Solver（暫名）

## 0. 摘要
這是一個在瀏覽器執行的 NLHE 6-max cash game GTO solver，供賽後研究使用。它分兩層：

- **翻前**：解 6 人翻前樹。規則限制成「最多 2 人看翻牌」。
- **翻後**：從翻前結果取得雙方範圍，指定翻牌後，用 DCFR 解出 HU 翻後的完整策略、EV 和 equity。

核心用 Rust 寫，同時編譯成 WASM（給網頁）和 native CLI。前端沿用 HH Stats Viewer 的技術棧：Vite、React、TypeScript。

**混合架構（2026-10-06 決定，見 §5.1）**：
- 轉牌和河牌 spot 在瀏覽器裡解。
- 翻牌 spot 的樹太大，放不進瀏覽器的 4GB，所以用本機 native CLI 解，結果匯出成檔案，再到網頁上查看。

## 1. 目標 / 非目標
**目標**
- 翻後 HU solver，結果和參考 solver 一致（§8.4）。可以收斂到指定的 exploitability。
- 6-max 翻前 solver，輸出每個位置、每條行動線的 13×13 範圍表。
- 支援 **rake**（%、cap、no flop no drop）。預設值用 GG NL10 實測的 5%、cap 3bb。
- 解之前先估算記憶體，超過預算時拒絕執行並說明原因。
- 手機能**查看**存檔的結果，但不保證手機能**求解**。

**非目標（v1 不做）**
- 翻後多人底池。
- Limp（包含 SB limp）。
- Ante、straddle、ICM、錦標賽。
- Node locking。
- 跨多個翻牌的 aggregate report。
- **即時輔助（RTA）**：GG 等平台禁止遊戲中使用 solver。本工具只做研究，不做牌桌疊加或自動讀取牌桌。

## 2. 使用流程
1. **翻前**：設定籌碼深度（預設 100bb）、rake、翻前尺寸 → 求解 → 瀏覽各位置的範圍表（例如 BTN open、BB 面對 BTN open）。
2. **選 spot**：選一條翻前行動線，例如 BTN open → BB call。系統自動帶入雙方範圍、底池和有效籌碼。範圍可以手動編輯。
3. **翻後**：選翻牌 → 顯示樹的大小和記憶體估算 → 求解（顯示進度和 exploitability）→ 逐節點查看策略、EV、equity、EQR。
4. **存檔**：結果存在 IndexedDB，也可以匯出、匯入 JSON。

## 3. 遊戲模型
### 3.1 參數（都可以設定）
- 6-max：UTG、HJ、CO、BTN、SB、BB。盲注 0.5 / 1bb，無 ante。
- 有效籌碼預設 100bb，範圍 20–200bb。每人籌碼相同。
- Rake：比例預設 5%，cap 預設 3bb，預設 no flop no drop。Jackpot fee 是可選項，預設關閉。
  - Rake 在 terminal 從贏家拿到的底池扣除。
  - 因為有 rake，遊戲不是零和。CFR 的收斂保證不嚴格成立，但實務上 PioSolver 和其他 solver 都這樣處理。這點要在 §8 用 exploitability 實測確認。

### 3.2 翻前動作樹
- **尺寸**：
  - Open 2.5bb，SB open 3bb。
  - 3Bet：IP 3×，OOP（盲注位）4×。
  - 4Bet 2.3×。
  - 5Bet 一律 all-in。
  - 任何加注額超過剩餘籌碼的 X%（預設 40%）時，改成 all-in。
- **不允許 limp**：UTG 到 SB 第一個入池的人，只能 raise 或 fold。
- **HU 規則（v1 的簡化）**：有人 call 之後，行動結束。所有還沒行動的玩家強制 fold，也就是沒有 squeeze、沒有第二個 cold call。因此每條 call 線都只會留下 2 人看翻牌。
  - 副作用：例如 CO open、BTN call 之後，盲注沒有防守機會，BTN 的 call 範圍會偏寬。
  - 這是已知偏差，UI 要明確告知使用者。v2 開放多人底池後移除這條規則。

### 3.3 翻後動作樹（HU）
- **轉牌、河牌 spot 的預設**：
  - 下注：check、33%、66%、100%、125% pot，三條街相同。
  - 加注：一種尺寸，加到 3× 對方的下注，另外加 all-in。每條街最多 3 次加注（可設定）。
  - Donk：允許 OOP 領先下注，尺寸和一般下注相同。
- **翻牌 spot 的預設**（2026-10-06 決定，見 §5.1）：
  - 翻牌：check、33%、66%、100%、125%，翻牌不能 donk。
  - 轉牌、河牌：check、66%、125%。
  - 加注：3× 加 all-in，每條街最多 1 次。
  - 所有設定都可以改，改了以後會重新估算記憶體。
- **All-in 門檻**：下注後剩餘籌碼少於底池的 Y%（預設 10%）時，這個下注改成 all-in。如果多個尺寸換算後的金額相同，合併成一個。

## 4. 演算法
### 4.1 翻後：DCFR
- 使用 Discounted CFR（Brown & Sandholm 2019），參數 α=1.5、β=0、γ=2，兩名玩家交替更新。
- **以手牌為向量**：每個節點一次處理整個範圍（最多 1326 組 combos）。Regret 和累積策略存成 `[actions × hands]` 的 f32 陣列。
- **Chance node**：轉牌、河牌全部列舉，不抽樣。
- **Suit isomorphism**：翻牌花色對稱的發牌共用子樹，例如單色翻牌下，其他三種花色的轉牌是等價的。
- **Showdown 計算**：每塊河牌的牌力事先排序。用前綴和算勝負，再扣除每張被擋掉的牌（card removal），所以每個 terminal 的計算量是 O(n) 而不是 O(n²)。
- **收斂指標**：exploitability = 雙方 best response 增益的平均，換算成底池的 %。預設目標 0.3% pot，另外設 iteration 上限。

### 4.2 翻前：6 人 CFR
- 翻前樹很小（幾百個節點），以 169 種起手牌類別為向量，做完整樹的 CFR，不需要 Monte Carlo 抽樣。
- 超過 2 人的 CFR 沒有收斂到 Nash 的理論保證（MonkerSolver 也有這個問題）。收斂指標改為：給定其他人的策略，每位玩家的 best response 增益（bb/100），要求低於門檻。
- **Terminal 估值**：
  - **Fold**：底池歸最後剩下的人，扣 rake（no flop no drop 時不扣）。
  - **All-in**：用預先算好的 169×169 HU equity 矩陣，乘上 combo 的相容權重（考慮 card removal）。
  - **看翻牌（v1）**：用 equity realization 模型。設 IP 的 realization 係數 R_IP、OOP 的係數 R_OOP，依位置和底池類型（SRP / 3BP / 4BP）設定。IP 分到的底池比例 = eq·R_IP / (eq·R_IP + (1−eq)·R_OOP)，再扣 rake。R 的預設值**待查文獻或用 M5 的翻後結果校正**。
  - **看翻牌（v2）**：改用翻後 solver，在代表性的翻牌子集上預先算好 EV 表。
- 已棄牌玩家的 card removal 先忽略，這是已知的近似。

## 5. 資料結構與記憶體
- **節點**：存在 arena 裡（`Vec<Node>`，用 index 互相連結）。策略資料放在一塊連續的 `Vec<f32>`，每個節點記錄自己的 offset。
- **記憶體估算**：先建一棵只有骨架、不配置資料的樹，統計 Σ(actions × hands × 8 bytes)，也就是 regret 加累積策略。UI 先顯示估算值，超過預算（預設 3GB）就拒絕求解。
- **可選壓縮**：regret 改用 i16、累積策略改用 u16，各自帶 scale，記憶體大約減半。精度影響要在 §8 實測。
- **WASM 限制**：wasm32 最多 4GB。memory64 不是所有瀏覽器都支援（Safari 狀態**未確認**），所以 v1 以 wasm32 為目標。
- **主要風險**：100bb SRP 三條街都用 4 種尺寸，加上加注和翻牌 donk，樹很可能超過 4GB。M2 要實測這組設定。如果超過，備案依序是：
  1. 開啟壓縮。
  2. 降低每條街的加注次數上限。
  3. 關閉翻牌 donk，或轉牌、河牌減少尺寸。這一項要先經過使用者同意。

### 5.1 M2 實測與決定（2026-10-06）
測試情境：BTN vs BB，100bb SRP（底池 5.5bb、有效籌碼 97.5bb），翻牌 Ks7d2c，雙方各約 530 組 combos。下表是用 `hexas tree` 估算的 f32 記憶體：

| 設定 | 記憶體 |
|---|---|
| 原設定：三條街各 4 種尺寸、最多 3 次加注、翻牌 donk | 132 GB |
| 原設定，但每條街最多 1 次加注 | 100 GB |
| 翻牌 4 種尺寸 / 轉牌、河牌 66,125 / 最多 1 次加注 / 翻牌不 donk | 14.8 GB |
| 每條街各 1 種尺寸 / 最多 1 次加注 | 4.1 GB |
| 轉牌 spot，原設定 | 0.39 GB |

決定：
- **平台採混合架構**。轉牌、河牌在瀏覽器解；翻牌用 native CLI 解（使用者電腦 31GB RAM、16 執行緒），結果匯出成檔案，再到網頁查看。
- **翻牌 spot 改用縮小後的預設**（見 §3.3）。開啟壓縮後約 7.4GB。

## 6. 架構
```
hexas-solver/
  crates/core/     cards, evaluator, tree, dcfr, preflop, exploitability（純 Rust，無 IO）
  crates/cli/      native 執行檔：tree / solve / compare；翻牌 spot 在這裡解並匯出結果檔
  crates/wasm/     wasm-bindgen API：estimate(config), solve(config, onProgress), query(path)
  web/             Vite + React + TS；solver 在 Web Worker 裡執行
  data/            預算好的 169×169 equity 矩陣（約 114KB binary）
```
- **多執行緒**：用 rayon 加 wasm-bindgen-rayon。這需要 SharedArrayBuffer，也就是頁面必須是 cross-origin isolated（COOP/COEP header）。
  - GitHub Pages 不能自訂 header，所以用 coi-serviceworker 補上。
  - 補不上的時候退回單執行緒。
- **Evaluator**：用 Rust 重寫 7-card evaluator（lookup table），驗證方式和 Stats Viewer 的 `src/equity/evaluator.ts` 相同。
- **參考實作**：b-inary/postflop-solver（Rust、DCFR、支援 WASM）。
  - 2026-10-06 確認：授權 AGPL-3.0-or-later；作者從 2023-10 起暫停開發，最後一次 commit 是 2024-07；repo 沒有封存。
  - 只把它當做**驗證用的對照組**和設計參考，不複製它的程式碼，避免整個專案被迫採用 AGPL。
  - 對照程式放在 repo 外的 `C:\Users\KH\ref\hexas-compare`，不會發布。結果見 §8.4。

## 7. UI
- **翻前範圍表**：6 個位置，13×13 格子，每格依行動頻率上色（fold / call / raise / all-in）。可以沿著行動線往下點。
- **Spot 設定**：選擇翻前行動線，範圍自動帶入（可編輯）；選翻牌（52 張牌的選擇器）；設定動作樹；顯示記憶體估算。
- **結果**：
  - 每個節點的策略格子、整體行動頻率、每組 combo 的 EV 和 equity，可依牌型類別篩選。
  - 轉牌報告：每張轉牌下的整體策略。
- **求解中**：顯示 iteration 數、exploitability 曲線、可以取消。
- **之後整合**：Stats Viewer 的手牌重播加一顆按鈕，透過 URL hash 把 spot 傳過來（位置、行動線、翻牌）。

## 8. 測試與驗收
1. **Evaluator**：窮舉全部 133,784,560 種 7 張牌組合，各牌型數量必須完全吻合已知值：
   - Straight flush 41,584
   - Four of a kind 224,848
   - Full house 3,473,184
   - Flush 4,047,644
   - Straight 6,180,020
   - Three of a kind 6,461,620
   - Two pair 31,433,400
   - One pair 58,627,800
   - High card 23,294,460
2. **CFR 正確性**：
   - Kuhn poker：P1 的 game value 收斂到 −1/18，exploitability 趨近 0。
   - Leduc hold'em：exploitability 收斂到 < 0.001 chip/hand（DCFR 的下降不保證每一步都單調）。
3. **河牌玩具局**：極化範圍（nuts + 空氣）對上抓詐牌，下注 pot。解析解是：下注範圍中詐唬佔 1/3，抓詐方跟注 50%（MDF）。容許誤差 ±1%。
4. **對照參考 solver**：選 3 個 spot（河牌、轉牌、翻牌各一），樹完全相同。要求：
   - 每個動作的頻率差 < 2%。
   - EV 差 < 0.5% pot。
   - 這項只在本機執行，CI 不跑。
   - **2026-10-06 結果：通過**。兩邊都解到 exploitability 0.02% pot。
     - 測試情境：
       - 範圍是 postflop-solver 範例裡的 OOP 和 IP 範圍。
       - 下注 50% / 100%，加注 3× 加 all-in，不自動改成 all-in，不合併尺寸。
       - 河牌、轉牌各跑不抽水和抽水（5%、cap 30）兩種；翻牌用小樹（只有 50%，籌碼 2 倍底池）。
     - EV 差距：全部 ≤ 0.011% pot。
     - 根節點和 IP 面對最常見下注的頻率差：河牌、轉牌都 ≤ 0.7%。
     - 翻牌的例外：「加注到 300」和「all-in 400」的分配差 3.4%，但兩者加總完全一樣（26.4%）。這兩個動作幾乎等價，均衡本來就不唯一。
     - 只比較均衡路徑上會到達的節點。像是 OOP 從不下 pot 時「IP 面對 pot 下注」這種節點，均衡策略不唯一，比較沒有意義。
     - 速度：postflop-solver 解到同樣精度約快 2.5 倍（轉牌 1.8 秒對 4.9 秒，小翻牌 13.6 秒對 33.4 秒），是之後效能優化的目標。
5. **不變量**：
   - 籌碼守恆（含 rake）。
   - 每個節點的策略加總為 1。
   - 把 spot 的花色互換後，結果必須相同（isomorphism 正確性）。
6. **翻前合理性**：
   - RFI 寬度隨位置單調變寬（UTG < HJ < CO < BTN）。
   - 10bb HU push/fold 的結果和公開的 Nash push/fold 表比對。
7. **效能**（目標值，M2 實測後修正）：
   - 8 執行緒下，河牌 spot < 1 秒、轉牌 spot < 10 秒。
   - 翻牌 SRP 到 0.5% pot 的時間：M2 定出基準。
   - **2026-10-06 實測**（使用者電腦，16 執行緒，BTN vs BB 100bb SRP 範圍）：
     - 河牌 spot：< 0.1 秒，達標。
     - 轉牌 spot（Ks7d2c5h，388MB）：
       - native 16 執行緒：400 iter 收斂到 0.38% pot，34 秒，未達 10 秒的目標。
       - 瀏覽器單執行緒：每 iter 約 0.7 秒。
     - 翻牌 spot：
       - 小樹（Ks7d2c，33/100、66、66，3.5GB）：0.38% pot，115 秒。
       - 預設樹（雙色 Ks7s2d，8.9GB）：275 iter 收斂到 0.489% pot，8.2 分鐘。結果檔 5.9MB，匯出花 8 秒。
       - 彩虹翻牌的預設樹（14.5GB）依比例推算約 13–14 分鐘，未實測：當時可用 RAM 只有 16GB。
     - DCFR 參數：γ 用 2 或 3、有沒有在 4 的次方時重置平均策略，收斂所需的 iteration 數都差不多（轉牌 450–475 iter 到 0.3% pot）。預設維持 γ=2，兩者都保留為 CLI 選項。
     - postflop-solver 解到同樣精度約快 2.5 倍（§8.4），主要差在 terminal 的計算。

## 9. Milestones
| # | 內容 | 驗收 |
|---|---|---|
| M0 | 安裝 Rust 和 wasm-pack（**需要使用者同意**）；建立 repo 骨架；CI | `cargo test` 和 `npm run build` 綠燈 |
| M1 | cards、evaluator、河牌 DCFR、exploitability、CLI | §8.1、§8.2、§8.3 |
| M2 | 轉牌和翻牌 chance node、isomorphism、記憶體估算、壓縮、rayon | §8.4、§8.5；實測 §3.3 尺寸的記憶體 |
| M3 | WASM 加 Web Worker，做最小 UI：範圍輸入、牌面、求解（轉牌、河牌）、看策略；匯入 native 解出的翻牌結果檔 | 瀏覽器結果和 native 一致；翻牌結果檔可以在網頁瀏覽 |
| M4 | 6-max 翻前 solver 加範圍表 UI | §8.6 |
| M5 | 翻前到翻後的流程、存檔、GitHub Pages 加 coi-serviceworker | 部署後可以多執行緒求解 |
| M6 | 整合 Stats Viewer；用翻後結果校正 realization 係數 | 從重播一鍵開啟 spot |

### 9.1 進度（2026-10-06）
- **M0、M1**：完成。
- **M2**：完成，但有兩項依決定延後。
  - 已完成：多條街的樹、記憶體估算、花色同構、rayon、§8.4 對照。
  - 16-bit 壓縮：延後。使用者電腦有 31GB，翻牌預設樹最大約 14.5GB（彩虹翻牌），不壓縮也放得下。
  - 效能：轉牌 spot 尚未達到 §8.7 的目標，見 §8.7 的實測。
- **M3**：完成，和原計畫有以下差異。
  - **沒有用 wasm-bindgen**：它的 CLI 在這台電腦的 GNU toolchain 下編不起來（缺 MinGW 的 dlltool）。改成 JSON 指令加 C ABI，見 `crates/wasm`，只需要 `wasm32-unknown-unknown` target。
  - **瀏覽器暫時單執行緒**：rayon 在 wasm32 上會自動退回單執行緒。M5 要開多執行緒時，需要 wasm-bindgen-rayon 或自己寫 worker pool，這會牽涉到安裝 MinGW 或 MSVC（需要使用者同意）。
  - **結果檔 `.hxs`**：存翻牌和轉牌的平均策略（u8 量化）以及翻牌節點每手牌的 EV。河牌不存，網頁走到河牌時用該節點的雙方 reach 當權重，在瀏覽器重解那個河牌子局。重解屬於 unsafe subgame solving，結果和整棵樹一起解的不會完全相同。
  - **SIMD**：WASM 開啟 simd128，比沒開快約 8%。
- **M4**：核心完成。
  - 已完成：169 類別、169×169 all-in equity 表、§3.2 的翻前樹、六人向量 CFR、網頁翻前頁面。
  - **equity 表**：Monte Carlo，每對類別 50 萬次抽樣、固定種子，誤差約 ±0.07%。存在 `crates/core/data/preflop_equity.bin`，用 `hexas gen-equity` 重新產生。
  - **收斂**：623 個節點，300 iter 時每位玩家的 best-response 增益 < 0.01 bb/100。native 約 3 秒，瀏覽器約 5 秒。
  - **100bb、rake 5%/3bb 的 RFI**：UTG 15.3%、HJ 20.3%、CO 26.5%、BTN 37.6%、SB 39.2%。
  - **已知偏差**：realization 模型只看原始 equity，低估同花連張這類「equity 不高但打得好」的牌，所以 BTN 範圍裡幾乎沒有 54s、65s。R_IP=1.0、R_OOP=0.85 是佔位值，等 M6 校正。
  - **§8.6 尚未完成**：HU push/fold 和公開 Nash 表的比對還沒做。目前的測試只驗證 RFI 隨位置單調變寬、AA 一定 open、72o 一定棄牌、best-response 增益很小。
- **M6 前置：realization 校正實驗**（2026-10-06）
  - 情境：BTN open、BB call，100bb SRP，範圍取自翻前解（R_IP=1.0、R_OOP=0.85）。用固定種子抽 10 個隨機翻牌。
  - 樹：翻牌 33/66/100/125、不 donk；轉牌和河牌只有 66%；每條街最多加注 1 次。每個翻牌解到約 1% pot，約 2 分鐘。
  - 結果（10 個翻牌加總）：
    - 原始 equity 分配，IP 份額 61.9%。
    - 實際求解的 EV，IP 份額 72.7%。
    - 以同一組範圍、逐對類別擬合，k = R_IP / R_OOP ≈ 1.76，即 R_IP=1 時 R_OOP≈0.57。
  - 但如果把 R_OOP=0.61 套用到翻前**所有**對抗組合，RFI 會變得不合理：UTG 36%、HJ 24%、CO 21%、BTN 83%、SB 15%，不再隨位置單調變寬。
    - 原因：實現率依底池類型和誰是加注者而不同。例如 3bet 底池裡 OOP 是加注者，實現率好得多；一個全域的 k 無法表達。
  - 結論：預設維持 1.0 / 0.85（產生的範圍合理），但它明顯低估了 SRP 中 IP 的優勢。
  - **待決定**：M6 要不要改成依情境設定 R（SRP 跟注方、3BP 加注方等），每個情境用翻後解校正。

- **M5**：部分完成。
  - 已完成：翻前樹的「看翻牌」終點可以一鍵把雙方範圍（帶權重）、底池和籌碼帶進翻後頁面。
  - 未完成：
    - GitHub Pages 部署：目前 repo 是 private，免費方案不能用 Pages。
    - coi-serviceworker 和瀏覽器多執行緒：需要先決定工具鏈，見 M3 的說明。
    - IndexedDB 存檔。

## 10. 風險
- **記憶體**：§3.3 的尺寸可能超過 4GB（見 §5），M2 是決策點。
- **翻前的近似**：realization 模型和 HU 強制規則會讓翻前範圍有偏差。UI 要明確標示「近似解」。
- **多人 CFR 不保證收斂**：用 best response 增益監控。
- **合規**：只做研究用途，不做任何即時輔助（RTA）功能。

## 11. 已確認的決定（2026-10-05）
1. 翻後加注：3× 對方下注，另外加 all-in。
2. 翻牌 donk：預設**開啟**，可以設定關閉。這會讓翻牌樹變大，計入 §5 的記憶體風險。
   - 2026-10-06 更新：翻牌 spot 的預設改成不 donk（§5.1）。轉牌和河牌 spot 仍然允許 donk。
3. 翻前尺寸：open 2.5bb（SB 3bb）、3Bet IP 3× / OOP 4×、4Bet 2.3×、5Bet all-in。
4. 專案名稱暫定 HEXAS Solver，資料夾為 `HEXAS_SOLVER`。
