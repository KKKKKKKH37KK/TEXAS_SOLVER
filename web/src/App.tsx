/** Placeholder shell until the WASM solver lands in M3. */
export function App() {
  return (
    <main>
      <h1>HEXAS Solver</h1>
      <p className="muted">6-max NLHE cash game solver（開發中，僅供賽後研究）。</p>
      <p className="muted small">
        cross-origin isolated：{String(globalThis.crossOriginIsolated ?? false)}（多執行緒求解需要為 true，M5 處理）
      </p>
    </main>
  );
}
