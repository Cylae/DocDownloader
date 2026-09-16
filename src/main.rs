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
    let connect_timeout = std::time::Duration::from_secs(10);
    let request_timeout = std::time::Duration::from_secs(cli.timeout);
    let retry_policy = docdownloader::network::retry::RetryPolicy {
        max_retries: cli.retries,
        ..Default::default()
    };
    let http_client =
        HttpClient::new(connect_timeout, request_timeout)?.with_retry_policy(retry_policy);
    let registry = Arc::new(ProviderRegistry::new());
    let cache = CacheManager::default_manager();

    let engine = Arc::new(DownloadEngine::with_options(
        http_client,
        registry.clone(),
        cache.clone(),
        cli.concurrency,
        cli.force,
        cli.no_resume,
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
            let out_path = cli.output.as_deref().or(cli.output_dir.as_deref());

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
            if let Some(first_page) = pub_info.pages.first()
                && let Some(best) = first_page.best_candidate()
                && let Some(geom) = best.geometry
            {
                println!(
                    "Best Discovered Quality: {}x{} px ({:?})",
                    geom.width, geom.height, best.asset_type
                );
            }
            let direct_pdf_status = match pub_info.direct_pdf_url {
                Some(ref url) => format!("Available ({url})"),
                None => "Unavailable".to_string(),
            };
            println!("Direct PDF: {direct_pdf_status}");
            let extraction_method = if pub_info.direct_pdf_url.is_some() {
                "Direct PDF download"
            } else {
                "High-resolution page assets"
            };
            println!("Extraction Method: {extraction_method}");
            if let Some(ref thumb) = pub_info.thumbnail_url {
                println!("Thumbnail: {thumb}");
            }

            let report = docdownloader::core::quality::QualityReport::from_publication(&pub_info);
            println!("\n{report}");
        }
        Commands::Batch(args) => {
            let content = std::fs::read_to_string(&args.file).map_err(|e| {
                DocDownloaderError::FileSystemError {
                    path: args.file.clone(),
                    reason: format!("Failed to read batch file: {e}"),
                }
            })?;

            let urls: Vec<String> = content
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|s| s.to_string())
                .collect();

            if urls.is_empty() {
                println!("No valid URLs found in {}", args.file.display());
                return Ok(());
            }

            println!(
                "=== Starting Batch Download of {} publication(s) ===",
                urls.len()
            );
            let mut succeeded = 0;
            let mut failed = 0;

            for (idx, raw_url) in urls.iter().enumerate() {
                println!("\n[{}/{}] Processing: {raw_url}", idx + 1, urls.len());
                let parsed_url = match Url::parse(raw_url) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("  Error: Invalid URL '{raw_url}': {e}");
                        failed += 1;
                        continue;
                    }
                };

                let listener = CliProgressReporter::new(cli.quiet);
                let out_dir = cli.output_dir.as_deref();

                match engine
                    .download(&parsed_url, out_dir, listener, cancel_rx.clone())
                    .await
                {
                    Ok(path) => {
                        println!("  Successfully saved: {}", path.display());
                        succeeded += 1;
                    }
                    Err(err) => {
                        eprintln!("  Failed to process {raw_url}: {err}");
                        failed += 1;
                    }
                }
            }

            println!("\n=== Batch Summary ===");
            println!("  Total:     {}", urls.len());
            println!("  Succeeded: {succeeded}");
            println!("  Failed:    {failed}");

            if failed > 0 {
                return Err(DocDownloaderError::InternalInvariantViolation {
                    reason: format!("Batch completed with {failed} failure(s)"),
                });
            }
        }
        Commands::Diagnostic(args) => {
            let parsed_url = Url::parse(&args.url).map_err(|e| DocDownloaderError::InvalidUrl {
                url: args.url.clone(),
                reason: e.to_string(),
            })?;

            let provider_name = match registry.find_provider(&parsed_url) {
                Some(p) => p.name(),
                None => "unknown",
            };

            let mut bundle =
                docdownloader::core::diagnostic::DiagnosticBundle::new(provider_name, &parsed_url);
            bundle.set_stage("ProbingProvider");

            match engine.inspect(&parsed_url).await {
                Ok(pub_info) => {
                    bundle.set_stage("MetadataResolved");
                    bundle.discovered_page_count = Some(pub_info.page_count);
                    bundle.direct_pdf_available = pub_info.direct_pdf_url.is_some();
                    bundle.selected_strategy = Some(if pub_info.direct_pdf_url.is_some() {
                        "DirectPdf".to_string()
                    } else {
                        "PageAssets".to_string()
                    });
                    bundle.record_http_status(200);
                }
                Err(err) => {
                    bundle.set_error(&err);
                }
            }

            match args.output {
                Some(out_path) => {
                    bundle.save_to_file(&out_path)?;
                    println!("Diagnostic bundle exported to: {}", out_path.display());
                }
                None => {
                    let json = bundle.to_json().map_err(|e| {
                        DocDownloaderError::InternalInvariantViolation {
                            reason: format!("Failed to serialize diagnostic: {e}"),
                        }
                    })?;
                    println!("{json}");
                }
            }
        }
        Commands::Cache(args) => {
            let action = args.action.unwrap_or(CacheAction::Status);
            match action {
                CacheAction::Status => {
                    let items = cache.list_cached_publications()?;
                    let total_bytes = cache.total_size_bytes();
                    let mb = total_bytes as f64 / (1024.0 * 1024.0);
                    println!("Cache Status:");
                    println!("  Location: {}", cache.base_dir().display());
                    println!("  Publications: {}", items.len());
                    println!("  Total Size: {:.2} MB ({} bytes)", mb, total_bytes);
                }
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
