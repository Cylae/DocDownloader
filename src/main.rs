use clap::Parser;
use std::sync::Arc;
use tokio::sync::watch;
use url::Url;

use docdownloader::cli::args::{CacheAction, Cli, Commands, DownloadArgs};
use docdownloader::cli::progress::CliProgressReporter;
use docdownloader::core::engine::DownloadEngine;
use docdownloader::core::error::DocDownloaderError;
use docdownloader::network::client::HttpClient;
use docdownloader::providers::ProviderRegistry;
use docdownloader::storage::cache::CacheManager;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Configure logging
    let filter = if cli.verbose {
        "docdownloader=debug,tower_http=debug"
    } else {
        "docdownloader=warn"
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();

    // Set up cancellation signal (Ctrl+C)
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nReceived interrupt signal. Gracefully cancelling active operations...");
            let _ = cancel_tx.send(true);
        }
    });

    let exit_code = match run_cli(cli, cancel_rx).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            err.exit_code()
        }
    };

    std::process::exit(exit_code);
}

async fn run_cli(cli: Cli, cancel_rx: watch::Receiver<bool>) -> Result<(), DocDownloaderError> {
    let http_client = HttpClient::default_client()?;
    let registry = Arc::new(ProviderRegistry::new());
    let cache = CacheManager::default_manager();

    let engine = Arc::new(DownloadEngine::new(
        http_client,
        registry.clone(),
        cache.clone(),
        cli.concurrency,
        cli.force,
    ));

    // Handle commands or default URL shorthand
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            if let Some(url) = cli.url {
                Commands::Download(DownloadArgs { url })
            } else {
                use clap::CommandFactory;
                Cli::command().print_help().map_err(|e| {
                    DocDownloaderError::InternalInvariantViolation {
                        reason: format!("Failed to print help: {e}"),
                    }
                })?;
                println!();
                return Ok(());
            }
        }
    };

    match command {
        Commands::Download(args) => {
            let parsed_url = Url::parse(&args.url).map_err(|e| DocDownloaderError::InvalidUrl {
                url: args.url.clone(),
                reason: e.to_string(),
            })?;

            let listener = CliProgressReporter::new(cli.quiet);
            let out_path = cli.output.as_deref();

            let result = engine
                .download(&parsed_url, out_path, listener, cancel_rx)
                .await?;

            if !cli.quiet {
                println!("Saved PDF: {}", result.display());
            }
        }
        Commands::Inspect(args) => {
            let parsed_url = Url::parse(&args.url).map_err(|e| DocDownloaderError::InvalidUrl {
                url: args.url.clone(),
                reason: e.to_string(),
            })?;

            let pub_info = engine.inspect(&parsed_url).await?;

            println!("Provider: {}", pub_info.provider);
            println!("Title: {}", pub_info.title);
            if let Some(ref author) = pub_info.author {
                println!("Author/Publisher: {author}");
            }
            println!("Publication ID: {}", pub_info.publication_id);
            println!("Pages: {}", pub_info.page_count);
            if let Some(geom) = pub_info.geometry {
                println!("Document Geometry: {}x{} pt", geom.width, geom.height);
            }
            if let Some(ref direct_pdf) = pub_info.direct_pdf_url {
                println!("Direct PDF Available: Yes ({direct_pdf})");
            } else {
                println!(
                    "Direct PDF Available: No (Reconstructing via high-resolution page assets)"
                );
            }
            if let Some(ref thumb) = pub_info.thumbnail_url {
                println!("Thumbnail: {thumb}");
            }
        }
        Commands::Cache(args) => {
            let action = args.action.unwrap_or(CacheAction::List);
            match action {
                CacheAction::List => {
                    let items = cache.list_cached_publications()?;
                    if items.is_empty() {
                        println!("Cache is empty.");
                    } else {
                        println!("Cached publications:");
                        for (prov, id, path) in items {
                            println!("  [{prov}] {id} ({})", path.display());
                        }
                    }
                }
                CacheAction::Clean { id, all } => {
                    if all {
                        cache.clean_all()?;
                        println!("Cleaned entire cache directory.");
                    } else if let Some(pub_id) = id {
                        // Find matching provider
                        let items = cache.list_cached_publications()?;
                        let mut found = false;
                        for (prov, item_id, _) in items {
                            if item_id == pub_id {
                                cache.clean_publication(&prov, &pub_id)?;
                                println!("Cleaned cache for publication: {pub_id}");
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            println!("Publication '{pub_id}' not found in cache.");
                        }
                    } else {
                        println!("Specify either --all or --id <publication_id>");
                    }
                }
            }
        }
        Commands::Serve(args) => {
            docdownloader::web::run_server(engine, &args.host, args.port).await?;
        }
    }

    Ok(())
}
