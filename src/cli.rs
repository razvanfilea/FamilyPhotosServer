use crate::http::AppStateRef;
use crate::model::user::User;
use crate::utils::password_hash::{generate_hash_from_password, generate_random_password};
use crate::{tasks, previews};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    commands: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    /// Manage users
    Users(UsersCommand),
    #[command(subcommand)]
    /// Manage Photos
    Photos(PhotosCommand),
}

#[derive(Subcommand)]
enum UsersCommand {
    /// Create a new user
    Create {
        #[arg(short, long)]
        /// The name used for login and the filesystem folder name
        user_id: String,
        #[arg(short, long)]
        /// The name visible to the user
        name: String,
        #[arg(short, long)]
        /// Random password will be generated if not provided
        password: Option<String>,
    },
    /// List all users and their respective photo count
    List,
    /// Remove an existing user
    Remove {
        #[arg(short, long)]
        user_id: String,
    },
}

#[derive(Subcommand)]
enum PhotosCommand {
    /// Trigger a manual scan of the filesystem
    ScanPhotos,
    /// Trigger a manual generation of previews
    GeneratePreviews,
}

#[derive(Subcommand)]
enum SessionsCommand {
    /// Clear all sessions
    Clear,
}

/**
 * @return true if the program should exit
 */
pub async fn run_cli(state: AppStateRef) -> bool {
    let cli = Cli::parse();

    let cmd = cli.commands;
    if cmd.is_none() {
        return false;
    }

    match cmd.unwrap() {
        Commands::Users(command) => user_commands(state, command).await,
        Commands::Photos(command) => photos_commands(state, command).await,
    };

    true
}

async fn user_commands(state: AppStateRef, command: UsersCommand) {
    match command {
        UsersCommand::Create {
            user_id,
            name,
            password,
        } => {
            let final_password = &password.unwrap_or_else(generate_random_password);
            let user = User {
                id: user_id,
                name,
                password_hash: generate_hash_from_password(final_password),
            };

            let user_result = state.users_repo.insert_user(&user).await;

            match user_result {
                Ok(_) => println!(
                    "User created with user name=\"{}\", name=\"{}\", password=\"{}\"",
                    user.id, user.name, final_password
                ),
                _ => eprintln!("Error creating user"),
            }
        }
        UsersCommand::List => {
            println!(
                "| {0: <12} | {1: <12} | {2: <12} |",
                "User Id", "Name", "Photos Count"
            );
            println!("+{0}+{0}|{0}+", "-".repeat(12 + 2));

            let users = state
                .users_repo
                .get_users()
                .await
                .expect("Failed to get users");

            for user in users {
                let count = state
                    .photos_repo
                    .get_photos_by_user(user.id.as_str())
                    .await
                    .expect("Failed to get photos count")
                    .len();

                println!(
                    "| {0: <12} | {1: <12} | {2: <12} |",
                    user.id, user.name, count
                );
            }
        }
        UsersCommand::Remove { user_id } => {
            println!(
                "Are you sure you want to delete the user {user_id}? Its files won't be affected. [y/N]"
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            let delete = input.to_lowercase().starts_with('y');

            if delete {
                match state.users_repo.delete_user(&user_id).await {
                    Ok(_) => println!("Deleted user with user id: {user_id}"),
                    _ => eprintln!("Failed to remove user with user name: {user_id}"),
                }
            }
        }
    }
}

async fn photos_commands(state: AppStateRef, command: PhotosCommand) {
    match command {
        PhotosCommand::ScanPhotos => tasks::scan_new_files(state).await,
        PhotosCommand::GeneratePreviews => match previews::generate_all_previews(state).await {
            Ok(_) => println!("Preview generation finished"),
            Err(e) => eprintln!("Preview generation failed: {e}"),
        },
    }
}
