import type { Aggregate } from './grid';
import { cellClass, pct, RANKS } from './grid';

interface Props {
  agg: Aggregate;
  colors: string[];
  selected: string | null;
  onSelect: (cls: string) => void;
}

/** 13×13 grid; each cell is split into the class's action frequencies and faded by its reach. */
export function RangeGrid({ agg, colors, selected, onSelect }: Props) {
  return (
    <div className="grid13" role="grid" aria-label="13×13 hand grid">
      {Array.from(RANKS).map((_, r) =>
        Array.from(RANKS).map((_, c) => {
          const cls = cellClass(r, c);
          const st = agg.byClass.get(cls);
          const w = st?.weight ?? 0;
          const shade = agg.maxWeight > 0 ? w / agg.maxWeight : 0;
          const title =
            st && w > 0
              ? `${cls}  ${st.combos} combos` +
                (st.freq.length ? '\n' + st.freq.map((f) => pct(f)).join(' / ') : '') +
                (st.ev !== null ? `\nEV ${st.ev.toFixed(2)} bb` : '')
              : `${cls}（不在範圍內）`;
          return (
            <button
              key={cls}
              type="button"
              className={`cell${selected === cls ? ' selected' : ''}${w > 0 ? '' : ' empty'}`}
              title={title}
              onClick={() => onSelect(cls)}
            >
              <span className="bars" style={{ opacity: 0.25 + 0.75 * shade }}>
                {w > 0 && st!.freq.length > 0
                  ? st!.freq.map((f, i) => <span key={i} style={{ width: `${f * 100}%`, background: colors[i] }} />)
                  : w > 0 && <span style={{ width: '100%', background: 'var(--a-range)' }} />}
              </span>
              <span className="label">{cls}</span>
            </button>
          );
        }),
      )}
    </div>
  );
}
