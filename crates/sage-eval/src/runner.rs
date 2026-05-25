//! Drive a `Reader` over labeled samples and aggregate metrics.

use std::sync::Arc;

use sage_core::{DocId, Query, Reader, ReaderGraph, Result, TenantId};

use crate::metrics::{f1_at_k, mrr, precision_at_k, recall_at_k};

#[derive(Clone, Debug)]
pub struct EvalSample {
    pub query: Query,
    pub ground_truth: Vec<DocId>,
}

#[derive(Clone, Debug, Default)]
pub struct EvalReport {
    pub samples: usize,
    pub recall_at_k: f32,
    pub precision_at_k: f32,
    pub f1_at_k: f32,
    pub mrr: f32,
    pub k: usize,
}

pub struct EvalRunner<R: Reader> {
    reader: Arc<R>,
    tenant: TenantId,
    k: usize,
}

impl<R: Reader> std::fmt::Debug for EvalRunner<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalRunner")
            .field("tenant", &self.tenant)
            .field("k", &self.k)
            .finish_non_exhaustive()
    }
}

impl<R: Reader> EvalRunner<R> {
    pub fn new(reader: Arc<R>, k: usize) -> Self {
        Self {
            reader,
            tenant: TenantId::DEFAULT,
            k,
        }
    }

    pub fn with_tenant(mut self, t: TenantId) -> Self {
        self.tenant = t;
        self
    }

    pub async fn run(
        &self,
        graph: &(dyn ReaderGraph + Sync),
        samples: &[EvalSample],
    ) -> Result<EvalReport> {
        if samples.is_empty() {
            return Ok(EvalReport {
                k: self.k,
                ..EvalReport::default()
            });
        }
        let mut sum_r = 0.0f32;
        let mut sum_p = 0.0f32;
        let mut sum_f = 0.0f32;
        let mut sum_m = 0.0f32;
        for s in samples {
            let out = self.reader.read(self.tenant, &s.query, graph).await?;
            let retrieved: Vec<DocId> = out.docs.iter().map(|(d, _)| *d).collect();
            sum_r += recall_at_k(&retrieved, &s.ground_truth, self.k);
            sum_p += precision_at_k(&retrieved, &s.ground_truth, self.k);
            sum_f += f1_at_k(&retrieved, &s.ground_truth, self.k);
            sum_m += mrr(&retrieved, &s.ground_truth);
        }
        let n = samples.len() as f32;
        Ok(EvalReport {
            samples: samples.len(),
            recall_at_k: sum_r / n,
            precision_at_k: sum_p / n,
            f1_at_k: sum_f / n,
            mrr: sum_m / n,
            k: self.k,
        })
    }
}
