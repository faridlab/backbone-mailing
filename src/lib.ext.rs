//! The module's extension (hand-written; ADR-0031): what the module adds beside its
//! generated services. The generated module and builder carry `ModuleExt` and
//! `ModuleBuilderExt` and dereference to them, so their fields read as `module.field`.

#[allow(unused_imports)]
use super::*;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    pub(crate) trace_clicks: crate::application::service::trace_click_ports::TraceClickSlot,
    pub(crate) trace_route_service:
        std::sync::Arc<crate::application::service::trace_route_service::TraceRouteService>,
}

/// State the builder adds.
pub struct ModuleBuilderExt {
}

impl Default for ModuleBuilderExt {
    fn default() -> Self {
        Self {
        }
    }
}

impl ModuleBuilderExt {
    /// Build the extension's state from what the generated build made.
    #[allow(unused_variables, clippy::redundant_clone)]
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let db_pool = parts.db_pool.clone();
        let trace_clicks =
            crate::application::service::trace_click_ports::TraceClickSlot::default();
        let trace_route_service = std::sync::Arc::new(
            crate::application::service::trace_route_service::TraceRouteService::new(
                db_pool.clone(),
                trace_clicks.clone(),
            ),
        );
        Ok(ModuleExt {
            trace_clicks,
            trace_route_service,
        })
    }
}

impl crate::ThisModule {
    /// Compose the short-link click seam for the public trace-route family
    /// (deny-by-default until called — the PhoneBookPort shape): the ONE
    /// registered implementation answers code attribution + click minting
    /// by wrapping the short-link module's public seams.
    pub fn set_trace_click_port(
        &self,
        port: std::sync::Arc<dyn crate::application::service::trace_click_ports::TraceClickPort>,
    ) {
        self.trace_clicks.install(port);
    }
    /// The public `/r/:code/m/:trace` trace-route family (click + open
    /// pixel + unsubscribe): a BARE capability mount throttled 120/min per
    /// code — merge at the site ROOT, next to the short-link module's own
    /// `/r/:code` group (distinct path shapes; no shadowing).
    pub fn public_trace_routes(&self) -> Router {
        crate::presentation::http::trace_routes::public_composer(
            self.trace_route_service.clone(),
        )
    }
}
