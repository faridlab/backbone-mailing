//! The SaleInvoicedAmountPort — mailing's billing-side seam for the
//! mass_mailing_sale bridge's winner metric (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The A/B winner axis `sale_invoiced_amount` ranks variants by the invoiced
//! total attributed to each variant's cited engagement SOURCE (MVX-4:
//! attribution keys on utm source, never on campaign, because one source may
//! legitimately serve several campaigns). Upstream computes that total with a
//! sudo'ed grouped read over the billing host's own tables
//! (`account.move.source_id`, `sum(amount_untaxed_signed)`); that read is
//! exactly the cross-schema raw-read class this port exists to close. This
//! module takes NO code dependency on the billing module — the read crosses
//! as a DECLARED seam:
//!
//! - this module owns the trait and the slot;
//! - the host service registers ONE implementation that composes the billing
//!   module's public query surface (deciding there what counts as invoiced:
//!   posted invoices citing the source, tax-excluded totals);
//! - attribution stays one-way: the port answers a number, it never writes,
//!   never spends, and never widens into a generic billing query surface.
//!
//! Deny-by-default (the TraceClickPort/PhoneBookPort shape): until a host
//! registers an implementation via
//! `MailingWriteService::with_sale_invoiced_amount_port(...)`, every call
//! fails with [`SaleInvoicedAmountError::NotComposed`] — and the promotion
//! step SKIPS the test loudly instead of promoting on a fake zero (a
//! zero-amount "winner" would be an arbitrary pick dressed as a ranking).

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

/// Why a billing-side call failed.
#[derive(Debug, thiserror::Error)]
pub enum SaleInvoicedAmountError {
    /// No [`SaleInvoicedAmountPort`] has been composed for this module —
    /// the deny-by-default refusal. Installing one via
    /// `MailingWriteService::with_sale_invoiced_amount_port` is the only
    /// cure.
    #[error("sale invoiced amount port not composed: {detail}")]
    NotComposed { detail: String },
    /// The billing backend failed. Deliberately opaque — billing-side
    /// detail never leaks through the seam.
    #[error("billing backend: {0}")]
    Backend(String),
}

/// The invoiced total attributed to one engagement source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceInvoicedAmount {
    pub source_id: Uuid,
    /// The tax-excluded invoiced total attributed to the source (the
    /// upstream `sum(amount_untaxed_signed)` shape). The host's
    /// implementation decides the exact invoice states that count as
    /// invoiced — this module only ranks the numbers it is handed.
    pub amount_untaxed_total: rust_decimal::Decimal,
}

/// The billing-side seam: ONE method by design — a read-only,
/// source-keyed invoiced total. Nothing else about billing crosses.
#[async_trait]
pub trait SaleInvoicedAmountPort: Send + Sync {
    /// The invoiced total attributed to `source_id`. Read-only: the
    /// implementation must never write, mint, or reserve anything on the
    /// billing side.
    async fn invoiced_amount_for_source(
        &self,
        source_id: Uuid,
    ) -> Result<SourceInvoicedAmount, SaleInvoicedAmountError>;
}

/// The deny-by-default implementation: the slot's initial tenant. Every
/// call refuses with [`SaleInvoicedAmountError::NotComposed`].
pub struct RefusingSaleInvoicedAmount;

#[async_trait]
impl SaleInvoicedAmountPort for RefusingSaleInvoicedAmount {
    async fn invoiced_amount_for_source(
        &self,
        source_id: Uuid,
    ) -> Result<SourceInvoicedAmount, SaleInvoicedAmountError> {
        Err(SaleInvoicedAmountError::NotComposed {
            detail: format!(
                "no SaleInvoicedAmountPort is installed; refusing the invoiced-amount lookup \
                 for source {source_id} — compose one via \
                 MailingWriteService::with_sale_invoiced_amount_port"
            ),
        })
    }
}

/// The test double: answers every source with the canned total, no
/// database. Exercises the ranking (distinct totals per source) and the
/// refusing default (leave `refuse: true`).
pub struct CannedSaleInvoicedAmount {
    pub refuse: bool,
    pub amount: rust_decimal::Decimal,
}

#[async_trait]
impl SaleInvoicedAmountPort for CannedSaleInvoicedAmount {
    async fn invoiced_amount_for_source(
        &self,
        source_id: Uuid,
    ) -> Result<SourceInvoicedAmount, SaleInvoicedAmountError> {
        if self.refuse {
            return Err(SaleInvoicedAmountError::NotComposed {
                detail: format!("canned refusal for source {source_id}"),
            });
        }
        Ok(SourceInvoicedAmount {
            source_id,
            amount_untaxed_total: self.amount,
        })
    }
}

/// A shared, swappable seam slot — how a host registers its composition
/// without the write service needing a constructor break. The service is
/// built over the slot (defaulting to
/// [`RefusingSaleInvoicedAmount`]); installs happen once at boot and every
/// promotion step sees the new port on the NEXT call.
#[derive(Clone)]
pub struct SaleInvoicedAmountSlot {
    inner: Arc<std::sync::RwLock<Arc<dyn SaleInvoicedAmountPort>>>,
}

impl SaleInvoicedAmountSlot {
    /// Install (replace) the active port.
    pub fn install(&self, port: Arc<dyn SaleInvoicedAmountPort>) {
        // A poisoned lock still holds the old value — recovering it beats
        // panicking every future call over one panicked writer.
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = port;
    }

    /// The currently installed port.
    pub fn current(&self) -> Arc<dyn SaleInvoicedAmountPort> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Default for SaleInvoicedAmountSlot {
    fn default() -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(Arc::new(RefusingSaleInvoicedAmount))),
        }
    }
}

#[async_trait]
impl SaleInvoicedAmountPort for SaleInvoicedAmountSlot {
    async fn invoiced_amount_for_source(
        &self,
        source_id: Uuid,
    ) -> Result<SourceInvoicedAmount, SaleInvoicedAmountError> {
        self.current().invoiced_amount_for_source(source_id).await
    }
}
