use scafra_macros::Config;

#[derive(Config)]
struct InvalidConfig {
    #[config(optional)]
    value: String,
}

fn main() {}
