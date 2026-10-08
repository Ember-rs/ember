use axum::Router;

/// One route as declared by a controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteMetadata {
    pub controller: &'static str,
    pub method: &'static str,
    pub prefix: &'static str,
    pub path: &'static str,
}

/// Link-time registration record emitted by the routes attribute.
pub struct ControllerRegistration {
    pub controller: &'static str,
    pub register: fn(Router) -> Router,
    pub routes: &'static [RouteMetadata],
}

inventory::collect!(ControllerRegistration);

/// Implemented by generated controller route adapters.
pub trait ControllerRoutes: Send + Sync + 'static {
    fn register_routes(router: Router) -> Router
    where
        Self: Default,
    {
        Self::register_routes_with(router, Self::default())
    }

    /// Registers this already constructed controller instance. Generated
    /// adapters override this method. Handwritten adapters retain the older
    /// default-constructed fallback when their type implements `Default`.
    fn register_routes_with(router: Router, _controller: Self) -> Router
    where
        Self: Default,
    {
        Self::register_routes(router)
    }

    /// Static routes used by graph-composed startup validation.
    fn route_metadata() -> &'static [RouteMetadata] {
        &[]
    }
}

/// Implemented by the controller attribute to make its URL prefix available
/// to the generated route table without runtime reflection.
pub trait ControllerPrefix {
    const PREFIX: &'static str;
}
