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
pub trait ControllerRoutes: Default + Send + Sync + 'static {
    fn register_routes(router: Router) -> Router;
}

/// Implemented by the controller attribute to make its URL prefix available
/// to the generated route table without runtime reflection.
pub trait ControllerPrefix {
    const PREFIX: &'static str;
}
