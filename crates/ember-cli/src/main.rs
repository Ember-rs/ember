mod cli;
mod commands;
mod filesystem;
mod generator;
mod templates;

fn main() -> anyhow::Result<()> {
    commands::run(cli::parse().command)
}

#[cfg(test)]
mod tests;
