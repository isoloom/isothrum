//! Sealing a run (`lifecycle.seal`): once its machines are set up and its ready conditions hold,
//! the runner sets the seal's param to `true`, records it, and runs the seal's steps (a second
//! `terraform apply` closing a cloud's `internet: false` networks at its firewalls, where root in a
//! guest can't undo it). A seal that fails fails the start: the run is never ready unsealed, and
//! its record says why (`not_sealed`). Every later operation replays the recorded `true` (the
//! run's params are its record's), so a later apply keeps the seal; a successful `down` forgets
//! it, so the next start begins unsealed, with the param's default.

use super::Running;
use crate::Result;
use crate::error::Error;
use crate::executor::{Cx, run_steps};
use crate::store::RunRecord;

/// The seal's param once the run is sealed.
const SEALED: &str = "true";

impl Running<'_> {
    /// Seals the run when its manifest says how: the param set and recorded first (a seal that
    /// fails half-way is still replayed by the `down`), then the steps.
    pub(crate) async fn seal(&self, cx: &mut Cx<'_>, record: &mut RunRecord) -> Result<()> {
        let Some(seal) = self.manifest().lifecycle.seal.as_ref() else {
            return Ok(());
        };
        let param = seal.param.to_string();
        let mut params = cx.vars.params().clone();
        params.insert(param.clone(), SEALED.to_owned());
        cx.vars.set_params(params);
        record.params.insert(param, SEALED.to_owned());
        self.runner.store.save(record)?;
        let sealed = run_steps(cx, "lifecycle.seal", seal.steps.as_slice()).await;
        sealed.map(drop).map_err(|e| Error::Unsealed {
            run: self.name().to_string(),
            reason: e.to_string(),
        })
    }

    /// Forgets the seal after a successful `down`: the next start begins unsealed.
    pub(crate) fn forget_seal(&self) -> Result<()> {
        let Some(seal) = self.manifest().lifecycle.seal.as_ref() else {
            return Ok(());
        };
        let Some(mut record) = self.runner.store.record(self.name())? else {
            return Ok(());
        };
        if record.params.remove(seal.param.as_str()).is_some() {
            self.runner.store.save(&record)?;
        }
        Ok(())
    }
}
