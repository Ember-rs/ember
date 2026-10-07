use scafra::prelude::*;

#[controller("/")]
struct FirstController;

#[routes]
impl FirstController {
    #[get("/same")]
    async fn same(&self) -> &'static str {
        "first"
    }
}

#[controller("/")]
struct SecondController;

#[routes]
impl SecondController {
    #[get("/same")]
    async fn same(&self) -> &'static str {
        "second"
    }
}

#[test]
fn duplicate_routes_are_reported_before_router_build() {
    let error = build_router().expect_err("duplicate routes must fail startup");
    let message = error.to_string();
    assert!(message.contains("duplicate route GET /same"), "{message}");
}
