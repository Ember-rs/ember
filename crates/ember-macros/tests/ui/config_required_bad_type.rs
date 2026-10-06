use ember_macros::Config;

#[derive(Config)]
struct InvalidConfig {
    #[config(required)]
    port: u16,
}

fn main() {}
