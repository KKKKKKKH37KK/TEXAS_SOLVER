//! JSON description of a spot, shared by the browser interface, the CLI and result files.

use crate::cards::parse_cards;
use crate::holdem::{BetSizes, Rake, Spot, TreeConfig};
use crate::range::Range;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SizesSpec {
    /// Pot fractions, e.g. [0.33, 0.66].
    pub bets: Vec<f64>,
    pub raise_mult: f64,
    pub max_raises: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpotSpec {
    pub board: String,
    /// Range text; ignored for a player whose `*_weights` are given.
    pub oop: String,
    pub ip: String,
    /// Explicit weights per combo (1326, `combo_index` order), e.g. reach from a parent solve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oop_weights: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_weights: Option<Vec<f32>>,
    pub pot: f64,
    pub stack: f64,
    /// Flop, turn, river. Omitted: the PRD preset for the board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<[SizesSpec; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donk: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rake_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rake_cap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allin_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isomorphism: Option<bool>,
}

impl SpotSpec {
    fn range(text: &str, weights: &Option<Vec<f32>>) -> Result<Range, String> {
        match weights {
            Some(w) if w.len() == crate::cards::NUM_COMBOS => Ok(Range { weights: w.clone() }),
            Some(w) => Err(format!("weights need 1326 entries, got {}", w.len())),
            None => text.parse::<Range>().map_err(|e| e.to_string()),
        }
    }

    pub fn to_spot(&self) -> Result<Spot, String> {
        let board = parse_cards(&self.board).map_err(|e| e.to_string())?;
        let mut cfg = TreeConfig::preset(self.pot, self.stack, board.len());
        if let Some(s) = &self.sizes {
            cfg.sizes = s.clone().map(|x| BetSizes {
                bets: x.bets,
                raise_mult: x.raise_mult,
                max_raises: x.max_raises,
                bet_allin: false,
            });
        }
        if let Some(d) = self.donk {
            cfg.oop_flop_bets = d;
        }
        cfg.rake = Rake {
            pct: self.rake_pct.unwrap_or(cfg.rake.pct),
            cap: self.rake_cap.unwrap_or(cfg.rake.cap),
        };
        if let Some(t) = self.allin_threshold {
            cfg.allin_threshold = t;
        }
        if let Some(i) = self.isomorphism {
            cfg.isomorphism = i;
        }
        Ok(Spot {
            board,
            ranges: [
                Self::range(&self.oop, &self.oop_weights)?,
                Self::range(&self.ip, &self.ip_weights)?,
            ],
            config: cfg,
        })
    }

    /// The spec of an existing configuration (sizes and switches written out in full).
    pub fn describe(spot: &Spot, oop: &str, ip: &str) -> SpotSpec {
        let c = &spot.config;
        SpotSpec {
            board: spot.board.iter().map(|x| x.to_string()).collect(),
            oop: oop.into(),
            ip: ip.into(),
            pot: c.start_pot,
            stack: c.eff_stack,
            sizes: Some(c.sizes.clone().map(|s| SizesSpec {
                bets: s.bets,
                raise_mult: s.raise_mult,
                max_raises: s.max_raises,
            })),
            donk: Some(c.oop_flop_bets),
            rake_pct: Some(c.rake.pct),
            rake_cap: Some(c.rake.cap),
            allin_threshold: Some(c.allin_threshold),
            isomorphism: Some(c.isomorphism),
            ..Default::default()
        }
    }
}
