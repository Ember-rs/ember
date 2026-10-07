use scafra_macros::Config;

#[derive(Config)]
#[config(required)]
struct InvalidConfig {
    value: String,
}

fn main() {}
