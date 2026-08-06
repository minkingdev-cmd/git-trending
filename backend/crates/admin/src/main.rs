use clap::{Parser, Subcommand};
use ght_core::config::Settings;
use ght_core::db;
use ght_core::users;

#[derive(Parser)]
#[command(name = "ght-admin", about = "GH Trending admin CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a user account (bootstrap; no invite code needed)
    CreateUser {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
    },
    /// Manage invite codes
    Invite {
        #[command(subcommand)]
        action: InviteAction,
    },
}

#[derive(Subcommand)]
enum InviteAction {
    /// Generate a new invite code
    Create {
        /// Number of times the code can be used
        #[arg(long, default_value_t = 1)]
        uses: i32,
    },
    /// List invite codes and usage
    List,
    /// Revoke an invite code
    Revoke { code: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    match cli.command {
        Command::CreateUser { username, password } => {
            let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)?;
            match users::create_user(&pool, &username, &hash, None).await {
                Ok(id) => println!("created user '{username}' (id={id})"),
                Err(users::UserError::Duplicate) => {
                    anyhow::bail!("username '{username}' already taken")
                }
            }
        }
        Command::Invite { action } => match action {
            InviteAction::Create { uses } => {
                anyhow::ensure!(uses > 0, "--uses must be positive");
                let code = users::create_invite(&pool, uses).await?;
                println!("{code}");
            }
            InviteAction::List => {
                let rows = users::list_invites(&pool).await?;
                if rows.is_empty() {
                    println!("(no invite codes)");
                }
                for r in rows {
                    let status = if r.revoked { "revoked" } else { "active" };
                    println!(
                        "{}\t{}/{} used\t{}",
                        r.code, r.used_count, r.max_uses, status
                    );
                }
            }
            InviteAction::Revoke { code } => {
                if users::revoke_invite(&pool, &code).await? {
                    println!("revoked {code}");
                } else {
                    anyhow::bail!("invite code '{code}' not found");
                }
            }
        },
    }
    Ok(())
}
