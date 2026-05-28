# Strategy Comparison — SAGE × patent-ai Integration

> 4 ingest strategies tested on the same 91-patent Peplink corpus,
> evaluated against `queries.json` (24 standard queries) and
> `queries_complexity.json` (42 graded queries).

## Build cost summary
| Strategy | Description | Docs | Triples | Time (s) | LLM tokens |
|---|---|---|---|---|---|
| A | zero-token structured | 91 | 5016 | 4.1 | 0 |
| C | full LLM (existing) | 82 | 5662 | 6000 | 2,700,000 |
| D | LLM abstract only | 91 | 0 | 1495.2 | 136,500 |

## Eval — queries.json (6 tiers)
| Strategy | tier 5 multi-hop MRR | tier 6 bridge MRR | overall MRR |
|---|---|---|---|
| A | 0.833 | 1.0 | 0.806 |
| C | 0.714 | 0.833 | 0.667 |
| D | 0.405 | 0.444 | 0.493 |

## Eval — queries_complexity.json (8 complexity tiers)
| Strategy | overall MRR | Prec@3 |
|---|---|---|
| A | 0.825 | 0.659 |
| C | 0.853 | 0.667 |
| D | 0.468 | 0.349 |

## Raw results JSON
```json
{
  "build_cost": {
    "A": {
      "strategy": "A",
      "docs": 91,
      "triples": 5016,
      "time_s": 4.1,
      "llm_tokens_est": 0
    },
    "D": {
      "strategy": "D",
      "docs": 91,
      "triples": 0,
      "time_s": 1495.2,
      "llm_tokens_est": 136500
    },
    "C": {
      "strategy": "C",
      "docs": 82,
      "triples": 5662,
      "time_s": 6000,
      "llm_tokens_est": 2700000,
      "_note": "Pre-existing /tmp/sage_peplink.sled"
    }
  },
  "eval": {
    "A": {
      "queries_k3": {
        "by_tier": {
          "tier_1": {
            "n": 4,
            "recall_at_k": 0.75,
            "mrr": 0.625,
            "precision_at_k": 0.25
          },
          "tier_2": {
            "n": 4,
            "recall_at_k": 0.078,
            "mrr": 1.0,
            "precision_at_k": 0.833
          },
          "tier_3": {
            "n": 3,
            "recall_at_k": 0.073,
            "mrr": 1.0,
            "precision_at_k": 1.0
          },
          "tier_4": {
            "n": 3,
            "recall_at_k": 0.017,
            "mrr": 0.333,
            "precision_at_k": 0.222
          },
          "tier_5": {
            "n": 7,
            "recall_at_k": 0.077,
            "mrr": 0.833,
            "precision_at_k": 0.714
          },
          "tier_6": {
            "n": 3,
            "recall_at_k": 0.054,
            "mrr": 1.0,
            "precision_at_k": 0.667
          }
        },
        "overall": {
          "n": 24,
          "recall_at_k": 0.179,
          "mrr": 0.806,
          "precision_at_k": 0.625
        }
      },
      "complexity_k3": {
        "by_tier": {
          "tier_1": {
            "n": 5,
            "recall_at_k": 0.056,
            "mrr": 0.733,
            "precision_at_k": 0.667
          },
          "tier_2": {
            "n": 5,
            "recall_at_k": 0.113,
            "mrr": 0.9,
            "precision_at_k": 0.8
          },
          "tier_3": {
            "n": 6,
            "recall_at_k": 0.099,
            "mrr": 0.833,
            "precision_at_k": 0.611
          },
          "tier_4": {
            "n": 6,
            "recall_at_k": 0.047,
            "mrr": 0.833,
            "precision_at_k": 0.5
          },
          "tier_5": {
            "n": 5,
            "recall_at_k": 0.08,
            "mrr": 0.8,
            "precision_at_k": 0.667
          },
          "tier_6": {
            "n": 5,
            "recall_at_k": 0.077,
            "mrr": 0.9,
            "precision_at_k": 0.8
          },
          "tier_7": {
            "n": 5,
            "recall_at_k": 0.067,
            "mrr": 0.8,
            "precision_at_k": 0.733
          },
          "tier_8": {
            "n": 5,
            "recall_at_k": 0.085,
            "mrr": 0.8,
            "precision_at_k": 0.533
          }
        },
        "overall": {
          "n": 42,
          "recall_at_k": 0.077,
          "mrr": 0.825,
          "precision_at_k": 0.659
        }
      }
    },
    "C": {
      "queries_k3": {
        "by_tier": {
          "tier_1": {
            "n": 4,
            "recall_at_k": 0.0,
            "mrr": 0.0,
            "precision_at_k": 0.0
          },
          "tier_2": {
            "n": 4,
            "recall_at_k": 0.065,
            "mrr": 0.833,
            "precision_at_k": 0.667
          },
          "tier_3": {
            "n": 3,
            "recall_at_k": 0.049,
            "mrr": 0.667,
            "precision_at_k": 0.667
          },
          "tier_4": {
            "n": 3,
            "recall_at_k": 0.033,
            "mrr": 0.556,
            "precision_at_k": 0.444
          },
          "tier_5": {
            "n": 7,
            "recall_at_k": 0.077,
            "mrr": 0.714,
            "precision_at_k": 0.714
          },
          "tier_6": {
            "n": 3,
            "recall_at_k": 0.045,
            "mrr": 0.833,
            "precision_at_k": 0.556
          }
        },
        "overall": {
          "n": 24,
          "recall_at_k": 0.05,
          "mrr": 0.667,
          "precision_at_k": 0.528
        }
      },
      "complexity_k3": {
        "by_tier": {
          "tier_1": {
            "n": 5,
            "recall_at_k": 0.051,
            "mrr": 0.8,
            "precision_at_k": 0.667
          },
          "tier_2": {
            "n": 5,
            "recall_at_k": 0.086,
            "mrr": 0.767,
            "precision_at_k": 0.6
          },
          "tier_3": {
            "n": 6,
            "recall_at_k": 0.095,
            "mrr": 0.833,
            "precision_at_k": 0.611
          },
          "tier_4": {
            "n": 6,
            "recall_at_k": 0.217,
            "mrr": 0.722,
            "precision_at_k": 0.556
          },
          "tier_5": {
            "n": 5,
            "recall_at_k": 0.113,
            "mrr": 0.9,
            "precision_at_k": 0.6
          },
          "tier_6": {
            "n": 5,
            "recall_at_k": 0.065,
            "mrr": 0.9,
            "precision_at_k": 0.667
          },
          "tier_7": {
            "n": 5,
            "recall_at_k": 0.262,
            "mrr": 0.9,
            "precision_at_k": 0.733
          },
          "tier_8": {
            "n": 5,
            "recall_at_k": 0.052,
            "mrr": 0.7,
            "precision_at_k": 0.533
          }
        },
        "overall": {
          "n": 42,
          "recall_at_k": 0.124,
          "mrr": 0.853,
          "precision_at_k": 0.667
        }
      }
    },
    "D": {
      "queries_k3": {
        "by_tier": {
          "tier_1": {
            "n": 4,
            "recall_at_k": 0.5,
            "mrr": 0.333,
            "precision_at_k": 0.167
          },
          "tier_2": {
            "n": 4,
            "recall_at_k": 0.07,
            "mrr": 0.833,
            "precision_at_k": 0.583
          },
          "tier_3": {
            "n": 3,
            "recall_at_k": 0.048,
            "mrr": 0.667,
            "precision_at_k": 0.667
          },
          "tier_4": {
            "n": 3,
            "recall_at_k": 0.017,
            "mrr": 0.222,
            "precision_at_k": 0.222
          },
          "tier_5": {
            "n": 7,
            "recall_at_k": 0.04,
            "mrr": 0.405,
            "precision_at_k": 0.286
          },
          "tier_6": {
            "n": 3,
            "recall_at_k": 0.026,
            "mrr": 0.444,
            "precision_at_k": 0.333
          }
        },
        "overall": {
          "n": 24,
          "recall_at_k": 0.121,
          "mrr": 0.493,
          "precision_at_k": 0.403
        }
      },
      "complexity_k3": {
        "by_tier": {
          "tier_1": {
            "n": 5,
            "recall_at_k": 0.057,
            "mrr": 0.8,
            "precision_at_k": 0.733
          },
          "tier_2": {
            "n": 5,
            "recall_at_k": 0.036,
            "mrr": 0.8,
            "precision_at_k": 0.533
          },
          "tier_3": {
            "n": 6,
            "recall_at_k": 0.031,
            "mrr": 0.361,
            "precision_at_k": 0.333
          },
          "tier_4": {
            "n": 6,
            "recall_at_k": 0.021,
            "mrr": 0.444,
            "precision_at_k": 0.222
          },
          "tier_5": {
            "n": 5,
            "recall_at_k": 0.021,
            "mrr": 0.2,
            "precision_at_k": 0.2
          },
          "tier_6": {
            "n": 5,
            "recall_at_k": 0.04,
            "mrr": 0.667,
            "precision_at_k": 0.4
          },
          "tier_7": {
            "n": 5,
            "recall_at_k": 0.032,
            "mrr": 0.467,
            "precision_at_k": 0.333
          },
          "tier_8": {
            "n": 5,
            "recall_at_k": 0.031,
            "mrr": 0.4,
            "precision_at_k": 0.333
          }
        },
        "overall": {
          "n": 42,
          "recall_at_k": 0.034,
          "mrr": 0.468,
          "precision_at_k": 0.349
        }
      }
    }
  }
}
```