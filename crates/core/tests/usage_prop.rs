//! Independent review of PR 119, kept as a permanent test: random sequences of stamped, unstamped and
//! pre-merged (child) usages through `Usage::add`. Invariant, class by
//! class: sum(by_model) == totals - unattributed; the cap is never
//! exceeded; a model's bucket holds only that model's tokens (oracle).

use std::collections::BTreeMap;

use harness_core::event::{MAX_USAGE_MODELS, ModelTokens, Usage};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn tokens(r: &mut Lcg) -> ModelTokens {
    let input = r.below(10_000);
    let read = r.below(input + 1);
    let write = r.below(input - read + 1);
    ModelTokens {
        input_tokens: input,
        output_tokens: r.below(2_000),
        cache_read_tokens: read,
        cache_write_tokens: write,
    }
}

fn stamped(model: &str, t: &ModelTokens) -> Usage {
    let mut u = Usage {
        input_tokens: t.input_tokens,
        output_tokens: t.output_tokens,
        cache_read_tokens: t.cache_read_tokens,
        cache_write_tokens: t.cache_write_tokens,
        ..Default::default()
    };
    u.stamp(model);
    u
}

fn classes(t: &ModelTokens) -> [u64; 4] {
    [
        t.input_tokens,
        t.output_tokens,
        t.cache_read_tokens,
        t.cache_write_tokens,
    ]
}

/// Oracle of what `add` must hold: buckets for admitted models, and the
/// unattributed remainder.
#[derive(Default)]
struct Oracle {
    buckets: BTreeMap<String, ModelTokens>,
    unattributed: [u64; 4],
}

impl Oracle {
    fn add_bucketed(&mut self, model: &str, t: &ModelTokens) {
        if let Some(b) = self.buckets.get_mut(model) {
            b.add(t);
        } else if self.buckets.len() < MAX_USAGE_MODELS {
            self.buckets.insert(model.to_string(), t.clone());
        } else {
            for (u, c) in self.unattributed.iter_mut().zip(classes(t)) {
                *u += c;
            }
        }
    }
    fn add_unstamped(&mut self, t: &ModelTokens) {
        for (u, c) in self.unattributed.iter_mut().zip(classes(t)) {
            *u += c;
        }
    }
}

#[test]
fn random_sequences_keep_the_breakdown_exact() {
    for seed in 0..400u64 {
        let mut r = Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let pool = 1 + r.below(30) as usize; // up to 30 models: crosses the cap
        let models: Vec<String> = (0..pool).map(|i| format!("p{}::m{i}", i % 3)).collect();
        let mut acc = Usage::default();
        let mut o = Oracle::default();
        for _ in 0..r.below(120) {
            match r.below(4) {
                0 | 1 => {
                    let m = &models[r.below(pool as u64) as usize];
                    let t = tokens(&mut r);
                    acc.add(&stamped(m, &t));
                    o.add_bucketed(m, &t);
                }
                2 => {
                    // unstamped: a provider not behind a router / old log
                    let t = tokens(&mut r);
                    let u = Usage {
                        input_tokens: t.input_tokens,
                        output_tokens: t.output_tokens,
                        cache_read_tokens: t.cache_read_tokens,
                        cache_write_tokens: t.cache_write_tokens,
                        model: Some(models[0].clone()),
                        ..Default::default()
                    };
                    acc.add(&u);
                    o.add_unstamped(&t);
                }
                _ => {
                    // a child's whole turn, itself capped, merged in
                    let mut child = Usage::default();
                    let mut child_o = Oracle::default();
                    for _ in 0..r.below(25) {
                        let m = &models[r.below(pool as u64) as usize];
                        let t = tokens(&mut r);
                        child.add(&stamped(m, &t));
                        child_o.add_bucketed(m, &t);
                    }
                    acc.add(&child);
                    for (m, t) in &child_o.buckets {
                        o.add_bucketed(m, t);
                    }
                    o.add_unstamped(&ModelTokens {
                        input_tokens: child_o.unattributed[0],
                        output_tokens: child_o.unattributed[1],
                        cache_read_tokens: child_o.unattributed[2],
                        cache_write_tokens: child_o.unattributed[3],
                    });
                }
            }
            assert!(acc.by_model.len() <= MAX_USAGE_MODELS, "seed {seed}: cap");
            assert_eq!(
                acc.by_model, o.buckets,
                "seed {seed}: a bucket holds another model's tokens"
            );
            let mut sum = [0u64; 4];
            for t in acc.by_model.values() {
                for (s, c) in sum.iter_mut().zip(classes(t)) {
                    *s += c;
                }
            }
            let totals = [
                acc.input_tokens,
                acc.output_tokens,
                acc.cache_read_tokens,
                acc.cache_write_tokens,
            ];
            for k in 0..4 {
                assert_eq!(
                    sum[k] + o.unattributed[k],
                    totals[k],
                    "seed {seed}: class {k}"
                );
            }
        }
    }
}

/// The 17th model is refused, not merged into a neighbour: its tokens show
/// up only as totals minus breakdown.
#[test]
fn the_seventeenth_model_is_refused_not_merged() {
    let mut u = Usage::default();
    for i in 0..MAX_USAGE_MODELS {
        u.add(&stamped(
            &format!("p::m{i:02}"),
            &ModelTokens {
                input_tokens: 1,
                output_tokens: 1,
                ..Default::default()
            },
        ));
    }
    let before = u.by_model.clone();
    u.add(&stamped(
        "p::zz-17th",
        &ModelTokens {
            input_tokens: 1_000_000,
            output_tokens: 7,
            cache_read_tokens: 5,
            cache_write_tokens: 3,
        },
    ));
    assert_eq!(u.by_model, before);
    assert_eq!(u.input_tokens, 1_000_000 + MAX_USAGE_MODELS as u64);
    assert_eq!(u.cache_read_tokens, 5);
}
