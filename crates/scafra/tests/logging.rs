use scafra::prelude::*;

#[logger]
struct ExampleService;

#[logger]
fn standalone_log_function() {
    info!(component = "ExampleService", "function logging works");
}

#[logger]
impl ExampleService {
    fn run(&self) {
        info!(operation = "run", "service logging works");
        warn!("warning logging works");
    }
}

#[test]
fn logger_attribute_and_logging_macros_are_available_from_prelude() {
    ExampleService.run();
    standalone_log_function();
}
