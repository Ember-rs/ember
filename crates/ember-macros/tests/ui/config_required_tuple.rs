use ember_macros::Config;

#[derive(Config)]
struct InvalidConfig(#[config(required)] String);

fn main() {}
