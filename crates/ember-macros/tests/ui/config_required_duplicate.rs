use ember_macros::Config;

#[derive(Config)]
struct InvalidConfig {
    #[config(required, required)]
    value: String,
}

fn main() {}
