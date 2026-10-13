//! The composer's per-mailing scope seam (hand-written; user-owned).
//!
//! The send sweep is a cron: it runs outside any request, claims due mailings across the whole
//! database and drives each one. A composer that fences the mailing tables per org unit needs
//! each mailing driven inside the scope of the unit it belongs to — its recipients resolved,
//! its traces minted and its winner promoted there, never across another unit's rows. The
//! module knows no units, so it hands each mailing's work to this port and the composer runs it
//! in that mailing's scope.
//!
//! The default runs the work as it is: a composition without a fence behaves exactly as before.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

/// One mailing's work, as the sweep hands it over.
pub type ScopedWork<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[async_trait]
pub trait MailingRunScope: Send + Sync {
    /// Run `work` in the scope of `mailing_id`'s unit. `Err` means the scope could not be
    /// opened; the work did not run.
    async fn within<'a>(&'a self, mailing_id: Uuid, work: ScopedWork<'a>) -> Result<(), String>;
}

/// Runs the work as it is (no fence to scope it to).
pub struct Unscoped;

#[async_trait]
impl MailingRunScope for Unscoped {
    async fn within<'a>(&'a self, _mailing_id: Uuid, work: ScopedWork<'a>) -> Result<(), String> {
        work.await;
        Ok(())
    }
}

/// The default seam.
pub fn unscoped() -> Arc<dyn MailingRunScope> {
    Arc::new(Unscoped)
}
