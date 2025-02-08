use std::{fs, net::Ipv4Addr, path::PathBuf};

use anyhow::Result;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use git2::Repository;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const PORT: u16 = 3344;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=debug,tower_http=debug,axum::rejection=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    let app = Router::new()
        .layer(TraceLayer::new_for_http())
        .route("/repo", post(create_repo))
        .route("/repo/{user}/{name}", get(handle_git))
        .route("/repo/{user}/{name}/{*path}", get(handle_dumb_protocol))
        .route("/repo/{user}/{name}/files", get(fetch_repo));

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;

    debug!("Started server on port {PORT}");
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct CreateRepo {
    user: String,
    name: String,
}

async fn create_repo(Json(payload): Json<CreateRepo>) -> Result<(), Error> {
    let CreateRepo { user, name } = payload;

    let path = PathBuf::from("repos").join(&user).join(&name);

    debug!("Creating repo {name} for {user}");

    Repository::init_bare(path)?;

    Ok(())
}

enum Error {
    Git(git2::Error),
    NotFound,
}

impl From<git2::Error> for Error {
    fn from(error: git2::Error) -> Self {
        Self::Git(error)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Error::Git(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Something went wrong when: {}", error),
            )
                .into_response(),
            Error::NotFound => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

async fn handle_git(Path((user, name)): Path<(String, String)>) -> Result<(), Error> {
    let path = PathBuf::from("repos").join(&user).join(&name);

    debug!("Handling {}", path.display());

    Ok(())
}

async fn handle_dumb_protocol(
    Path((user, name, path)): Path<(String, String, String)>,
) -> Result<Vec<u8>, Error> {
    let path = PathBuf::from("repos").join(&user).join(&name).join(path);

    debug!("Handling dumb protocol: {}", path.display());

    let res = fs::read(path).map_err(|_| Error::NotFound)?;

    Ok(res)
}

async fn fetch_repo(Path((user, name)): Path<(String, String)>) -> Result<(), Error> {
    let path = PathBuf::from("repos").join(&user).join(&name);

    let repo = Repository::open_bare(path)?;

    repo.head()?.peel_to_tree()?;

    Ok(())
}
