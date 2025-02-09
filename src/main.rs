use std::{fs, net::Ipv4Addr, path::PathBuf};

use anyhow::Result;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use git2::{BlameOptions, Repository};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, decompression::RequestDecompressionLayer, trace::TraceLayer,
};
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
        .route("/repo", post(create_repo))
        .route("/repo/{user}/{name}", get(handle_git))
        .route("/repo/{user}/{name}/{*path}", get(handle_dumb_protocol))
        .route("/repo/{user}/{name}/files", get(fetch_repo))
        .route("/repo/{user}/{name}/branches", get(get_branches))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(RequestDecompressionLayer::new())
                .layer(CompressionLayer::new()),
        );

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

    let mut path = PathBuf::from("repos").join(&user).join(&name);
    path.set_extension("git");

    debug!("Creating repo {name} for {user}");

    Repository::init_bare(path)?;

    Ok(())
}

#[derive(Debug)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Node {
    File {
        name: String,
        commit: String,
        message: String,
        modified: i64,
    },
    Directory {
        name: String,
        childs: Vec<Node>,
    },
}

async fn fetch_repo(Path((user, name)): Path<(String, String)>) -> Result<Json<Node>, Error> {
    let path = PathBuf::from("repos").join(&user).join(&name);

    let repo = Repository::open_bare(path)?;

    let tree = repo.head()?.peel_to_tree()?;

    let mut root = Vec::new();

    process_tree(&repo, &tree, &mut root, "")?;

    Ok(Json(Node::Directory {
        name: "root".to_string(),
        childs: root,
    }))
}

fn process_tree<P: AsRef<std::path::Path>>(
    repo: &Repository,
    tree: &git2::Tree,
    parent: &mut Vec<Node>,
    prefix: P,
) -> Result<(), Error> {
    for entry in tree {
        let name = entry.name().unwrap().to_string();

        let full_path = prefix.as_ref().join(&name);

        let node = if let Some(subtree) = entry.to_object(&repo)?.as_tree() {
            let mut childs = Vec::new();

            process_tree(repo, subtree, &mut childs, &full_path)?;

            Node::Directory { name, childs }
        } else {
            let mut blame_options = BlameOptions::new();

            let blame = repo.blame_file(&full_path, Some(&mut blame_options))?;
            let hunk = blame.get_index(0).unwrap();
            let commit_id = hunk.final_commit_id();
            let commit = repo.find_commit(commit_id)?;
            let message = commit.message().unwrap().to_string();
            let modified = commit.committer().when().seconds();

            Node::File {
                name,
                commit: commit_id.to_string(),
                message,
                modified,
            }
        };

        parent.push(node);
    }

    Ok(())
}

async fn get_branches(
    Path((user, name)): Path<(String, String)>,
) -> Result<Json<Vec<String>>, Error> {
    let path = PathBuf::from("repos").join(&user).join(&name);

    let repo = Repository::open_bare(path)?;

    let mut branches = Vec::new();

    for branch in repo.branches(None)? {
        let (branch, _) = branch?;

        branches.push(branch.name()?.unwrap().to_string());
    }

    Ok(Json(branches))
}
