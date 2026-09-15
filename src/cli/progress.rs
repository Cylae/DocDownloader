use console::Term;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crate::core::engine::ProgressListener;
use crate::core::job::JobState;

pub struct CliProgressReporter {
    is_tty: bool,
    quiet: bool,
    progress_bar: Option<ProgressBar>,
    completed_count: AtomicU32,
    total_bytes: AtomicU64,
}

impl CliProgressReporter {
    pub fn new(quiet: bool) -> Arc<Self> {
        let is_tty = Term::stdout().is_term() && !quiet;

        let pb = if is_tty {
            let bar = ProgressBar::new(100);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("#>-"),
            );
            Some(bar)
        } else {
            None
        };

        Arc::new(Self {
            is_tty,
            quiet,
            progress_bar: pb,
            completed_count: AtomicU32::new(0),
            total_bytes: AtomicU64::new(0),
        })
    }
}

impl ProgressListener for CliProgressReporter {
    fn on_state_change(&self, state: &JobState) {
        if self.quiet {
            return;
        }

        match state {
            JobState::Downloading { completed, total } => {
                self.completed_count.store(*completed, Ordering::Relaxed);
                if let Some(ref pb) = self.progress_bar {
                    pb.set_length(*total as u64);
                    pb.set_position(*completed as u64);
                    pb.set_message("Downloading pages...");
                } else {
                    println!("Downloading: {completed}/{total} pages...");
                }
            }
            JobState::BuildingPdf => {
                if let Some(ref pb) = self.progress_bar {
                    pb.set_message("Assembling PDF document...");
                } else {
                    println!("Building PDF...");
                }
            }
            JobState::ValidatingPdf => {
                if let Some(ref pb) = self.progress_bar {
                    pb.set_message("Validating PDF structure...");
                } else {
                    println!("Validating PDF...");
                }
            }
            JobState::Completed { output_path, total_pages, bytes } => {
                if let Some(ref pb) = self.progress_bar {
                    pb.finish_and_clear();
                }
                let mb = *bytes as f64 / (1024.0 * 1024.0);
                println!(
                    "Success: Reconstructed {} pages ({:.2} MB) -> {}",
                    total_pages,
                    mb,
                    output_path.display()
                );
            }
            JobState::Failed { error } => {
                if let Some(ref pb) = self.progress_bar {
                    pb.finish_and_clear();
                }
                eprintln!("Error: {error}");
            }
            _ => {
                if !self.is_tty {
                    println!("{}", state.description());
                } else if let Some(ref pb) = self.progress_bar {
                    pb.set_message(state.description());
                }
            }
        }
    }

    fn on_page_completed(&self, page_index: u32, total_pages: u32, bytes: u64, from_cache: bool) {
        let current = self.completed_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);

        if let Some(ref pb) = self.progress_bar {
            pb.set_length(total_pages as u64);
            pb.set_position(current as u64);
            let source_tag = if from_cache { "[cached]" } else { "" };
            pb.set_message(format!("Page {page_index}/{total_pages} {source_tag}"));
        } else if !self.quiet {
            // Log every 10% or on last page when running non-interactive
            let interval = (total_pages / 10).max(1);
            if current % interval == 0 || current == total_pages {
                let percent = (current as f64 / total_pages as f64) * 100.0;
                println!("Progress: {current}/{total_pages} ({percent:.0}%)");
            }
        }
    }

    fn on_log_message(&self, message: &str) {
        if !self.quiet {
            if let Some(ref pb) = self.progress_bar {
                pb.println(message);
            } else {
                println!("{message}");
            }
        }
    }
}
