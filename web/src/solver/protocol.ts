/** Types shared by the UI, the worker and the wasm JSON interface (crates/wasm/src/lib.rs). */

export interface StreetSizes {
  /** Pot fractions, e.g. [0.33, 0.66]. */
  bets: number[];
  raiseMult: number;
  maxRaises: number;
}

export interface SpotIn {
  board: string;
  oop: string;
  ip: string;
  pot: number;
  stack: number;
  /** Flop, turn, river. Omitted: the PRD preset for the board. */
  sizes?: [StreetSizes, StreetSizes, StreetSizes];
  donk?: boolean;
  rakePct?: number;
  rakeCap?: number;
  allinThreshold?: number;
  isomorphism?: boolean;
}

export interface Estimate {
  nodes: number;
  actionNodes: number;
  bytes: number;
  hands: [number, number];
}

export interface Report {
  iteration: number;
  ev: [number, number];
  exploitability: number;
  exploitabilityPct: number;
}

export type ActionKind = 'fold' | 'check' | 'call' | 'bet' | 'raise' | 'allin';

export interface ActionInfo {
  kind: ActionKind;
  amount: number;
  label: string;
}

/** A path step: take action `a`, or deal card `c` ("5h"). */
export type PathStep = { a: number } | { c: string };

export interface NodeView {
  kind: 'action' | 'chance' | 'fold' | 'showdown';
  player: number | null;
  actions: ActionInfo[];
  board: string[];
  pot: number;
  stacks: [number, number];
  street: [number, number];
  hands: [string[], string[]];
  reach: [number[], number[]];
  /** [action][hand] for the player to act. */
  strategy: number[] | null;
  ev: [number[], number[]] | null;
  dealable: string[];
}

export type Request =
  | { cmd: 'estimate'; spot: SpotIn }
  | { cmd: 'create'; spot: SpotIn }
  | { cmd: 'step'; n: number }
  | { cmd: 'report' }
  | { cmd: 'view'; path: PathStep[]; ev?: boolean }
  | { cmd: 'destroy' };

export type ToWorker =
  | { id: number; type: 'call'; req: Request }
  | { id: number; type: 'solve'; maxIter: number; targetPct: number }
  | { type: 'stop' };

export type FromWorker =
  | { id: number; ok: true; reply: unknown }
  | { id: number; ok: false; error: string }
  /** `report` is null between (expensive) exploitability measurements. */
  | { type: 'progress'; iteration: number; report: Report | null; elapsed: number };
