use ember_macros::Config;

#[derive(Config)]
struct InvalidConfig {
    #[config(required = true)]
    value: String,
}

fn main() {}
