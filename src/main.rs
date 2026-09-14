use clap::{Parser, Subcommand};
use codexy::{auth, config, proxy};
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Login,
    Serve,
    Status,
    Logout,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let home = config::home()?;
    let path = home.join("auth.json");
    match cli.command {
        Command::Login => auth::login(&path).await?,
        Command::Logout => {
            match std::fs::remove_file(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            };
            println!("Logged out.");
        }
        Command::Status => match auth::Credentials::load(&path) {
            Ok(c) => println!(
                "Logged in; access token {}.",
                if c.expires_at > auth::now() {
                    "valid"
                } else {
                    "requires refresh"
                }
            ),
            Err(_) => println!("Not logged in."),
        },
        Command::Serve => {
            proxy::serve(
                config::Config::load(&cli.config.unwrap_or_else(|| home.join("config.toml")))?,
                path,
            )
            .await?
        }
    }
    Ok(())
}
