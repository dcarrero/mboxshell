//! CLI entry point for `mboxShell`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

use mboxshell::i18n;
use mboxshell::index::{builder, reader as index_reader};

#[derive(Parser)]
#[command(name = "mboxshell", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// MBOX file or directory to open
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Force rebuild index even if one already exists
    #[arg(short, long, global = true)]
    force: bool,

    /// Verbose logging (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Language (en, es). Defaults to system locale.
    #[arg(long, value_name = "LANG")]
    lang: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Open a file in the TUI
    Open {
        path: PathBuf,
    },
    /// Index an MBOX file
    Index {
        path: PathBuf,
    },
    /// Show statistics
    Stats {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Search messages
    Search {
        path: PathBuf,
        query: String,
        #[arg(long)]
        json: bool,
    },
    /// Export messages
    Export {
        path: PathBuf,
        #[arg(short, long, default_value = "eml")]
        format: String,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        query: Option<String>,
    },
    /// Merge multiple MBOX files
    Merge {
        inputs: Vec<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value = "true")]
        dedup: bool,
    },
    /// Extract all attachments
    Attachments {
        path: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Generate shell completions
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Generate a man page
    Manpage,
}

/// Detect language early from --lang arg or system env, before clap processes --help.
fn detect_lang_early() -> i18n::Lang {
    // Check --lang flag in raw args
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--lang" {
            if let Some(code) = args.get(i + 1) {
                if let Some(lang) = i18n::Lang::from_code(code) {
                    return lang;
                }
            }
        }
        if let Some(code) = args[i].strip_prefix("--lang=") {
            if let Some(lang) = i18n::Lang::from_code(code) {
                return lang;
            }
        }
    }
    i18n::detect_system_lang()
}

/// Build a localized clap Command using i18n strings.
fn build_localized_command() -> clap::Command {
    let mut cmd = Cli::command();
    cmd = cmd
        .about(i18n::app_about())
        .long_about(i18n::app_long_about())
        .after_help(i18n::app_after_help());

    // Localize subcommands
    let subcommands: Vec<clap::Command> = cmd
        .get_subcommands()
        .map(|sub| {
            let mut s = sub.clone();
            match s.get_name() {
                "open" => { s = s.about(i18n::help_cmd_open()); }
                "index" => { s = s.about(i18n::help_cmd_index()); }
                "stats" => { s = s.about(i18n::help_cmd_stats()); }
                "search" => { s = s.about(i18n::help_cmd_search()); }
                "export" => { s = s.about(i18n::help_cmd_export()); }
                "merge" => { s = s.about(i18n::help_cmd_merge()); }
                "attachments" => { s = s.about(i18n::help_cmd_attachments()); }
                "completions" => { s = s.about(i18n::help_cmd_completions()); }
                "manpage" => { s = s.about(i18n::help_cmd_manpage()); }
                _ => {}
            }
            s
        })
        .collect();

    // Replace subcommands (clear and re-add)
    for sub in subcommands {
        cmd = cmd.mut_subcommand(sub.get_name(), |_| sub.clone());
    }

    cmd
}

fn main() -> anyhow::Result<()> {
    // Detect language BEFORE clap parsing so --help is localized
    let lang = detect_lang_early();
    i18n::set_lang(lang);

    // Build localized command and parse
    let cmd = build_localized_command();
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    // Load configuration
    let config = mboxshell::config::load_config();

    // Configure logging: stderr + optional log file
    let log_level = match cli.verbose {
        0 => config.general.log_level.as_str(),
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    setup_logging(log_level, &config);

    let force = cli.force;

    match cli.command {
        Some(Commands::Index { path }) => cmd_index(&path, force),
        Some(Commands::Stats { path, json }) => cmd_stats(&path, json, force),
        Some(Commands::Open { path }) => cmd_open(&path, force),
        None => {
            if let Some(path) = cli.file {
                cmd_open(&path, force)
            } else {
                cmd_open_interactive()
            }
        }
        Some(Commands::Search { path, query, json }) => cmd_search(&path, &query, json, force),
        Some(Commands::Export {
            path,
            format,
            output,
            query,
        }) => cmd_export(&path, &format, &output, query.as_deref(), force),
        Some(Commands::Merge {
            inputs,
            output,
            dedup,
        }) => cmd_merge(&inputs, &output, dedup),
        Some(Commands::Attachments { path, output }) => cmd_attachments(&path, &output, force),
        Some(Commands::Completions { shell }) => cmd_completions(shell),
        Some(Commands::Manpage) => cmd_manpage(),
    }
}

/// Set up tracing with stderr output and optional file logging.
fn setup_logging(level: &str, config: &mboxshell::config::Config) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    // Try to set up file logging
    let log_dir = mboxshell::config::cache_dir(config);
    if std::fs::create_dir_all(&log_dir).is_ok() {
        let file_appender = tracing_appender::rolling::never(&log_dir, "mboxshell.log");
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(file_appender);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer)
            .with(file_layer)
            .init();
    } else {
        // Fall back to stderr only
        tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer)
            .init();
    }
}

/// Generate shell completions and print to stdout.
fn cmd_completions(shell: clap_complete::Shell) -> anyhow::Result<()> {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "mboxshell", &mut std::io::stdout());
    Ok(())
}

/// Generate a man page and print to stdout.
fn cmd_manpage() -> anyhow::Result<()> {
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    std::io::Write::write_all(&mut std::io::stdout(), &buf)?;
    Ok(())
}

/// Index an MBOX file and print statistics.
fn cmd_index(path: &Path, force: bool) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }

    let file_size = std::fs::metadata(path)?.len();
    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {} [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{eta}})",
                i18n::msg_indexing()
            ))
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let start = Instant::now();

    let entries = builder::build_index(
        path,
        force,
        Some(&|current, total| {
            pb.set_length(total);
            pb.set_position(current);
        }),
    )?;

    pb.finish_and_clear();

    let elapsed = start.elapsed();
    let idx_size = builder::index_file_size(path);

    print_stats_table(path, file_size, &entries, elapsed, idx_size);

    Ok(())
}

/// Show statistics for an MBOX file.
fn cmd_stats(path: &Path, json: bool, force: bool) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }

    let file_size = std::fs::metadata(path)?.len();

    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {} [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}}",
                i18n::msg_building_index()
            ))
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let start = Instant::now();
    let entries = builder::build_index(
        path,
        force,
        Some(&|current, total| {
            pb.set_length(total);
            pb.set_position(current);
        }),
    )?;
    pb.finish_and_clear();
    let elapsed = start.elapsed();
    let idx_size = builder::index_file_size(path);

    if json {
        print_stats_json(path, file_size, &entries, elapsed, idx_size)?;
    } else {
        print_stats_table(path, file_size, &entries, elapsed, idx_size);
    }

    Ok(())
}

fn cmd_open(path: &Path, force: bool) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }
    mboxshell::tui::run_tui(path.to_path_buf(), force)
}

fn cmd_open_interactive() -> anyhow::Result<()> {
    eprintln!("{}", i18n::err_no_file_given());
    Ok(())
}

/// Search messages in an MBOX file and print results.
fn cmd_search(path: &Path, query: &str, json: bool, force: bool) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }

    // Build/load index
    let entries = builder::build_index(path, force, None)?;

    let pb = ProgressBar::new(entries.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Searching [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let (_parsed_query, results) = mboxshell::search::execute(
        path,
        &entries,
        query,
        Some(&|current, total| {
            pb.set_length(total as u64);
            pb.set_position(current as u64);
            true
        }),
    )?;

    pb.finish_and_clear();

    if json {
        print_search_results_json(&entries, &results)?;
    } else {
        print_search_results_table(&entries, &results);
    }

    Ok(())
}

/// Export messages from an MBOX file.
fn cmd_export(
    path: &Path,
    format: &str,
    output: &Path,
    query: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }

    let entries = builder::build_index(path, force, None)?;
    let mut store = mboxshell::store::reader::MboxStore::open(path)?;

    // Filter by query if provided
    let indices: Vec<usize> = if let Some(q) = query {
        let (_, results) = mboxshell::search::execute(path, &entries, q, None)?;
        results
    } else {
        (0..entries.len()).collect()
    };

    let selected: Vec<&mboxshell::model::mail::MailEntry> =
        indices.iter().map(|&i| &entries[i]).collect();

    println!(
        "  Exporting {} message(s) as {} to {}",
        selected.len(),
        format,
        output.display()
    );

    let pb = ProgressBar::new(selected.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Exporting [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    match format {
        "eml" => {
            std::fs::create_dir_all(output)?;
            let paths = mboxshell::export::eml::export_multiple_eml(
                &mut store,
                &selected,
                output,
                &|current, _total| {
                    pb.set_position(current as u64);
                },
            )?;
            pb.finish_and_clear();
            println!("  Exported {} .eml file(s)", paths.len());
        }
        "csv" => {
            let csv_path = if output.extension().is_some() {
                output.to_path_buf()
            } else {
                output.join("export.csv")
            };
            if let Some(parent) = csv_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            mboxshell::export::csv::export_csv(&selected, &csv_path, None)?;
            pb.finish_and_clear();
            println!("  Exported CSV to {}", csv_path.display());
        }
        "txt" | "text" => {
            std::fs::create_dir_all(output)?;
            let mut count = 0usize;
            for (i, entry) in selected.iter().enumerate() {
                pb.set_position(i as u64);
                let body = store.get_message(entry)?.clone();
                mboxshell::export::text::export_text(entry, &body, output)?;
                count += 1;
            }
            pb.finish_and_clear();
            println!("  Exported {} .txt file(s)", count);
        }
        _ => {
            anyhow::bail!(
                "Unknown export format '{}'. Supported: eml, csv, txt",
                format
            );
        }
    }

    Ok(())
}

/// Merge multiple MBOX files into one.
fn cmd_merge(inputs: &[PathBuf], output: &Path, dedup: bool) -> anyhow::Result<()> {
    for input in inputs {
        if !input.exists() {
            anyhow::bail!("{}: {}", i18n::err_file_not_found(), input.display());
        }
    }

    let pb = ProgressBar::new(inputs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Merging [{bar:40.cyan/blue}] {pos}/{len} files")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let stats = mboxshell::export::mbox::merge_mbox_files(
        inputs,
        output,
        dedup,
        &|current, _total, _name| {
            pb.set_position(current as u64);
        },
    )?;

    pb.finish_and_clear();

    use humansize::{format_size, BINARY};
    println!();
    println!("  Merge complete:");
    println!("  {:<25} {}", "Input files", stats.input_files);
    println!("  {:<25} {}", "Total messages", stats.total_messages);
    if dedup {
        println!(
            "  {:<25} {}",
            "Duplicates removed", stats.duplicates_removed
        );
    }
    println!(
        "  {:<25} {}",
        "Output size",
        format_size(stats.output_size, BINARY)
    );
    println!("  {:<25} {}", "Output file", output.display());
    println!();

    Ok(())
}

/// Extract all attachments from an MBOX file.
fn cmd_attachments(path: &Path, output: &Path, force: bool) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{}: {}", i18n::err_file_not_found(), path.display());
    }

    let entries = builder::build_index(path, force, None)?;
    let mut store = mboxshell::store::reader::MboxStore::open(path)?;

    let with_att: Vec<&mboxshell::model::mail::MailEntry> =
        entries.iter().filter(|e| e.has_attachments).collect();

    if with_att.is_empty() {
        println!("  No messages with attachments found.");
        return Ok(());
    }

    println!(
        "  Extracting attachments from {} message(s)",
        with_att.len()
    );

    let pb = ProgressBar::new(with_att.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} Extracting [{bar:40.cyan/blue}] {pos}/{len}")
            .expect("valid template")
            .progress_chars("#>-"),
    );

    let paths = mboxshell::export::attachment::export_bulk_attachments(
        &mut store,
        &with_att,
        output,
        &|current, _total| {
            pb.set_position(current as u64);
        },
    )?;

    pb.finish_and_clear();
    println!(
        "  Extracted {} attachment(s) to {}",
        paths.len(),
        output.display()
    );

    Ok(())
}

/// Print search results as a human-readable table.
fn print_search_results_table(entries: &[mboxshell::model::mail::MailEntry], results: &[usize]) {
    use humansize::{format_size, BINARY};

    println!();
    println!("  {} result(s)", results.len());
    println!();

    if results.is_empty() {
        return;
    }

    println!(
        "  {:<4} {:<17} {:<25} {:<40} {:>8}",
        "#", "Date", "From", "Subject", "Size"
    );
    println!("  {}", "-".repeat(98));

    for (i, &idx) in results.iter().enumerate() {
        let entry = &entries[idx];
        let date = entry.date.format("%Y-%m-%d %H:%M").to_string();
        let from = if entry.from.display_name.is_empty() {
            &entry.from.address
        } else {
            &entry.from.display_name
        };
        let from_trunc: String = from.chars().take(24).collect();
        let subj_trunc: String = entry.subject.chars().take(39).collect();
        let size = format_size(entry.length, BINARY);

        println!(
            "  {:<4} {:<17} {:<25} {:<40} {:>8}",
            i + 1,
            date,
            from_trunc,
            subj_trunc,
            size
        );
    }
    println!();
}

/// Print search results as JSON.
fn print_search_results_json(
    entries: &[mboxshell::model::mail::MailEntry],
    results: &[usize],
) -> anyhow::Result<()> {
    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|&idx| {
            let e = &entries[idx];
            serde_json::json!({
                "index": idx,
                "date": e.date.to_rfc3339(),
                "from": {
                    "address": e.from.address,
                    "display_name": e.from.display_name,
                },
                "to": e.to.iter().map(|a| serde_json::json!({
                    "address": a.address,
                    "display_name": a.display_name,
                })).collect::<Vec<_>>(),
                "subject": e.subject,
                "message_id": e.message_id,
                "size": e.length,
                "has_attachments": e.has_attachments,
                "labels": e.labels,
            })
        })
        .collect();

    let output = serde_json::json!({
        "result_count": results.len(),
        "results": items,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print statistics in a human-readable table.
fn print_stats_table(
    path: &Path,
    file_size: u64,
    entries: &[mboxshell::model::mail::MailEntry],
    elapsed: std::time::Duration,
    idx_size: u64,
) {
    use humansize::{format_size, BINARY};

    println!();
    println!("  {:<20} {}", i18n::msg_file(), path.display());
    println!(
        "  {:<20} {}",
        i18n::msg_file_size(),
        format_size(file_size, BINARY)
    );
    println!("  {:<20} {}", i18n::msg_message_count(), entries.len());

    if let Some((min, max)) = index_reader::date_range(entries) {
        println!(
            "  {:<20} {} — {}",
            i18n::msg_date_range(),
            min.format("%Y-%m-%d"),
            max.format("%Y-%m-%d")
        );
    }

    if idx_size > 0 {
        println!(
            "  {:<20} {}",
            i18n::msg_index_size(),
            format_size(idx_size, BINARY)
        );
    }

    println!("  {:<20} {:.2?}", i18n::msg_indexing_time(), elapsed);

    let with_att = index_reader::count_with_attachments(entries);
    println!(
        "  {:<20} {} ({:.1}%)",
        i18n::msg_with_attachments(),
        with_att,
        if entries.is_empty() {
            0.0
        } else {
            with_att as f64 / entries.len() as f64 * 100.0
        }
    );

    let top = index_reader::top_senders(entries, 10);
    if !top.is_empty() {
        println!();
        println!("  {}:", i18n::msg_top_senders());
        for (sender, count) in &top {
            println!("    {count:>6}  {sender}");
        }
    }
    println!();
}

/// Print statistics as JSON.
fn print_stats_json(
    path: &Path,
    file_size: u64,
    entries: &[mboxshell::model::mail::MailEntry],
    elapsed: std::time::Duration,
    idx_size: u64,
) -> anyhow::Result<()> {
    let date_range = index_reader::date_range(entries).map(|(min, max)| {
        serde_json::json!({
            "oldest": min.to_rfc3339(),
            "newest": max.to_rfc3339(),
        })
    });

    let top = index_reader::top_senders(entries, 10);
    let top_json: Vec<serde_json::Value> = top
        .iter()
        .map(|(sender, count)| {
            serde_json::json!({
                "sender": sender,
                "count": count,
            })
        })
        .collect();

    let stats = serde_json::json!({
        "file": path.to_string_lossy(),
        "file_size": file_size,
        "message_count": entries.len(),
        "date_range": date_range,
        "index_size": idx_size,
        "indexing_time_ms": elapsed.as_millis(),
        "with_attachments": index_reader::count_with_attachments(entries),
        "top_senders": top_json,
    });

    println!("{}", serde_json::to_string_pretty(&stats)?);
    Ok(())
}
