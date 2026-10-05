import { useState } from 'react';
import type { SpotIn } from '../solver/protocol';
import { boardLen, preset, toSizes, type TreeForm } from './presets';

// BTN open vs BB call, 100bb SRP (the PRD §5.1 measurement spot).
const DEFAULT_OOP =
  '22-99,A2s-AJs,K2s-KJs,Q2s-QJs,J4s-JTs,T6s-T9s,96s+,85s+,74s+,63s+,53s+,43s,A2o-AJo,K7o-KJo,Q8o-QJo,J8o-JTo,T8o+,97o+,87o,76o';
const DEFAULT_IP = '22+,A2s+,K2s+,Q4s+,J6s+,T6s+,96s+,85s+,75s+,64s+,54s,A2o+,K8o+,Q9o+,J9o+,T8o+,98o';

export interface SolveSettings {
  targetPct: number;
  maxIter: number;
}

interface Props {
  busy: boolean;
  onEstimate: (spot: SpotIn) => void;
  onSolve: (spot: SpotIn, s: SolveSettings) => void;
  onError: (msg: string) => void;
}

const STREETS = ['翻牌 Flop', '轉牌 Turn', '河牌 River'];

export function SpotForm({ busy, onEstimate, onSolve, onError }: Props) {
  const [board, setBoard] = useState('Ks7d2c5h');
  const [oop, setOop] = useState(DEFAULT_OOP);
  const [ip, setIp] = useState(DEFAULT_IP);
  const [pot, setPot] = useState(5.5);
  const [stack, setStack] = useState(97.5);
  const [tree, setTree] = useState<TreeForm>(preset(4));
  const [rakePct, setRakePct] = useState(5);
  const [rakeCap, setRakeCap] = useState(3);
  const [targetPct, setTargetPct] = useState(0.5);
  const [maxIter, setMaxIter] = useState(500);

  const n = boardLen(board);
  const firstStreet = Math.max(0, n - 3);

  // A flop spot and a turn/river spot have different presets: switch when the street changes.
  const [presetFor, setPresetFor] = useState(n === 3 ? 3 : 4);
  const kind = n === 3 ? 3 : 4;
  if (kind !== presetFor) {
    setPresetFor(kind);
    setTree(preset(kind));
  }

  const spot = (): SpotIn | null => {
    try {
      return {
        board: board.replace(/\s/g, ''),
        oop,
        ip,
        pot,
        stack,
        sizes: toSizes(tree),
        donk: tree.donk,
        rakePct: rakePct / 100,
        rakeCap,
      };
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      return null;
    }
  };

  const setBets = (i: number, v: string) => {
    const bets = [...tree.bets] as TreeForm['bets'];
    bets[i] = v;
    setTree({ ...tree, bets });
  };
  const setRaises = (i: number, v: number) => {
    const maxRaises = [...tree.maxRaises] as TreeForm['maxRaises'];
    maxRaises[i] = v;
    setTree({ ...tree, maxRaises });
  };

  return (
    <section className="card form">
      <h2>局面設定</h2>
      <div className="row">
        <label>
          公牌 Board
          <input value={board} onChange={(e) => setBoard(e.target.value)} placeholder="Ks7d2c" size={12} />
        </label>
        <label>
          底池 Pot (bb)
          <input type="number" step="0.5" value={pot} onChange={(e) => setPot(Number(e.target.value))} />
        </label>
        <label>
          有效籌碼 Stack (bb)
          <input type="number" step="0.5" value={stack} onChange={(e) => setStack(Number(e.target.value))} />
        </label>
        <label>
          抽水 Rake %
          <input type="number" step="0.5" value={rakePct} onChange={(e) => setRakePct(Number(e.target.value))} />
        </label>
        <label>
          上限 Cap (bb)
          <input type="number" step="0.5" value={rakeCap} onChange={(e) => setRakeCap(Number(e.target.value))} />
        </label>
      </div>
      <label className="block">
        OOP 範圍（先行動，例如 BB）
        <textarea value={oop} onChange={(e) => setOop(e.target.value)} rows={2} spellCheck={false} />
      </label>
      <label className="block">
        IP 範圍（後行動，例如 BTN）
        <textarea value={ip} onChange={(e) => setIp(e.target.value)} rows={2} spellCheck={false} />
      </label>

      <details>
        <summary>
          下注樹 Bet tree <span className="muted small">（加注 = 對方下注 × {tree.raiseMult}，另含 all-in）</span>
        </summary>
        <table className="sizes">
          <thead>
            <tr>
              <th />
              <th>下注 % pot</th>
              <th>每條街加注上限</th>
            </tr>
          </thead>
          <tbody>
            {STREETS.map((s, i) =>
              i < firstStreet ? null : (
                <tr key={s}>
                  <th scope="row">{s}</th>
                  <td>
                    <input value={tree.bets[i]} onChange={(e) => setBets(i, e.target.value)} size={16} />
                  </td>
                  <td>
                    <input
                      type="number"
                      min={0}
                      max={10}
                      value={tree.maxRaises[i]}
                      onChange={(e) => setRaises(i, Number(e.target.value))}
                    />
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
        <div className="row">
          <label>
            加注倍數 Raise ×
            <input
              type="number"
              step="0.5"
              value={tree.raiseMult}
              onChange={(e) => setTree({ ...tree, raiseMult: Number(e.target.value) })}
            />
          </label>
          {n === 3 && (
            <label className="check">
              <input type="checkbox" checked={tree.donk} onChange={(e) => setTree({ ...tree, donk: e.target.checked })} />
              翻牌允許 OOP 領先下注 (donk)
            </label>
          )}
          <button type="button" className="link" onClick={() => setTree(preset(n))}>
            套用 PRD 預設
          </button>
        </div>
        {n === 3 && (
          <p className="muted small">
            翻牌局面的樹通常需要數 GB，瀏覽器上限約 3 GB；完整翻牌建議用本機 CLI 解（PRD §5.1）。
          </p>
        )}
      </details>

      <div className="row actions">
        <label>
          目標 exploitability (% pot)
          <input type="number" step="0.1" value={targetPct} onChange={(e) => setTargetPct(Number(e.target.value))} />
        </label>
        <label>
          最多 iteration
          <input type="number" step="100" value={maxIter} onChange={(e) => setMaxIter(Number(e.target.value))} />
        </label>
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            const s = spot();
            if (s) onEstimate(s);
          }}
        >
          估算記憶體
        </button>
        <button
          type="button"
          className="primary"
          disabled={busy}
          onClick={() => {
            const s = spot();
            if (s) onSolve(s, { targetPct, maxIter });
          }}
        >
          求解
        </button>
      </div>
    </section>
  );
}
