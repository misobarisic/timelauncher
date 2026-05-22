use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use comfy_table::modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, Table};
use compact_str::CompactString;
use console::{Emoji, style};
use fd_lock::RwLock;
use indicatif::{ProgressBar, ProgressStyle};

static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍 ", "");
static CHECK: Emoji<'_, '_> = Emoji("✅ ", "");
static CROSS: Emoji<'_, '_> = Emoji("❌ ", "");
static GAMEPAD: Emoji<'_, '_> = Emoji("🎮 ", "");

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RunRecord {
    name: CompactString,
    command: CompactString,
    duration_ms: u64,
    exit_code: Option<i32>,
    timestamp: CompactString,
}

#[derive(ValueEnum, Clone, Debug)]
enum SortBy {
    Name,
    Time,
    Sessions,
}

#[derive(Parser)]
#[command(name = "timelauncher", version, about = "Time your games and apps")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a program or an alias
    Run {
        name: CompactString,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command_parts: Vec<String>,
    },
    /// Manage command aliases
    Alias {
        #[command(subcommand)]
        action: AliasAction,
    },
    /// Show detailed history for a specific program
    Info { name: CompactString },
    /// Show a summary table of all programs
    Summary {
        #[arg(short, long, value_enum, default_value_t = SortBy::Time)]
        sort: SortBy,
    },
}

#[derive(Subcommand)]
enum AliasAction {
    Add {
        name: CompactString,
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
    List,
    Remove {
        name: CompactString,
    },
}

struct ProgramStats {
    count: u64,
    total_time_ms: u64,
    first_played: CompactString,
    last_played: CompactString,
}

struct Playtime(u64);

impl fmt::Display for Playtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_secs = self.0 / 1000;
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        match (days, hours, minutes) {
            (d, h, m) if d > 0 => write!(f, "{}d {}h {}m", d, h, m),
            (0, h, m) if h > 0 => write!(f, "{}h {}m", h, m),
            (0, 0, m) if m > 0 => write!(f, "{}m {}s", m, seconds),
            _ => write!(f, "{}s", seconds),
        }
    }
}

fn get_data_paths() -> Result<(PathBuf, PathBuf)> {
    let mut path =
        dirs::data_local_dir().ok_or_else(|| anyhow!("Could not find data directory"))?;
    path.push("timelauncher");
    fs::create_dir_all(&path).context("Failed to create data directory")?;

    Ok((path.join("history.jsonl"), path.join("aliases.json")))
}

fn load_aliases(path: &Path) -> Result<HashMap<CompactString, CompactString>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(path).context("Failed to read aliases file")?;
    serde_json::from_str(&content).context("Failed to parse aliases.json")
}

fn save_aliases(path: &Path, aliases: &HashMap<CompactString, CompactString>) -> Result<()> {
    let content = serde_json::to_string_pretty(aliases)?;
    fs::write(path, content).context("Failed to save aliases")
}

fn save_record_jsonl(path: &Path, record: &RunRecord) -> Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;

    let mut lock = RwLock::new(file);
    let mut locked_file = lock.write()?;

    let json_line = serde_json::to_string(record)?;
    writeln!(locked_file, "{}", json_line)?;
    locked_file
        .flush()
        .context("Failed to flush records to disk")?;

    Ok(())
}

fn load_records_jsonl(path: &Path) -> Result<Vec<RunRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut buf = String::new();
    let mut line_count = 0;

    while reader.read_line(&mut buf)? > 0 {
        line_count += 1;
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            let record: RunRecord = serde_json::from_str(trimmed)
                .with_context(|| format!("Failed to parse JSON on line {}", line_count))?;
            records.push(record);
        }
        buf.clear();
    }
    Ok(records)
}

fn format_ts(ts: &str) -> String {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%d.%m.%Y.").to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

fn manage_alias(action: AliasAction, alias_path: &Path) -> Result<()> {
    let mut aliases = load_aliases(alias_path)?;
    match action {
        AliasAction::Add { name, command } => {
            let full_cmd = command.join(" ");
            aliases.insert(name.clone(), full_cmd.clone().into());
            save_aliases(alias_path, &aliases)?;
            println!(
                "{} Alias added: {} -> {}",
                CHECK,
                style(name).yellow(),
                style(full_cmd).dim()
            );
        }
        AliasAction::List => {
            if aliases.is_empty() {
                println!("No aliases found.");
                return Ok(());
            }
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_header(vec!["Name", "Command"]);
            for (name, cmd) in aliases {
                table.add_row(vec![Cell::new(name).fg(Color::Yellow), Cell::new(cmd)]);
            }
            println!("{table}");
        }
        AliasAction::Remove { name } => {
            if aliases.remove(&name).is_some() {
                save_aliases(alias_path, &aliases)?;
                println!("{} Removed alias: {}", CHECK, name);
            } else {
                println!("{} Alias '{}' not found.", CROSS, name);
            }
        }
    }
    Ok(())
}

fn run_command(
    name: CompactString,
    command_parts: Vec<String>,
    history_path: &Path,
    alias_path: &Path,
) -> Result<()> {
    let aliases = load_aliases(alias_path)?;
    let base_cmd_str = aliases
        .get(&name)
        .map(|s| s.as_str())
        .unwrap_or(name.as_str());

    let mut args = shlex::split(base_cmd_str)
        .ok_or_else(|| anyhow!("Invalid shell escaping in command/alias string"))?;
    args.extend(command_parts);

    if args.is_empty() {
        return Err(anyhow!("No command provided."));
    }

    println!(
        "{} Launching: {} ({})",
        GAMEPAD,
        style(&name).yellow().bold(),
        style(&args[0]).dim()
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] Active session: {msg}")?,
    );
    pb.set_message(style(&name).cyan().to_string());
    pb.enable_steady_tick(Duration::from_millis(100));

    let start = Instant::now();
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .spawn()
        .with_context(|| format!("Failed to launch executable: '{}'", args[0]))?;

    let status = child.wait()?;
    let duration_ms = start.elapsed().as_millis() as u64;
    pb.finish_and_clear();

    let record = RunRecord {
        name,
        command: args.join(" ").into(),
        duration_ms,
        exit_code: status.code(),
        timestamp: Local::now().to_rfc3339().into(),
    };

    save_record_jsonl(history_path, &record)?;

    println!(
        "{} Session ended. Playtime: {}",
        CHECK,
        style(Playtime(record.duration_ms)).bold().cyan()
    );
    Ok(())
}

fn show_info(name: CompactString, history_path: &Path) -> Result<()> {
    let all_records = load_records_jsonl(history_path)?;
    let filtered: Vec<_> = all_records.into_iter().filter(|r| r.name == name).collect();

    if filtered.is_empty() {
        println!("No records found for: {}", name);
        return Ok(());
    }

    let total_ms: u64 = filtered.iter().map(|r| r.duration_ms).sum();
    let first = filtered
        .iter()
        .min_by_key(|r| &r.timestamp)
        .unwrap()
        .timestamp
        .clone();
    let last = filtered
        .iter()
        .max_by_key(|r| &r.timestamp)
        .unwrap()
        .timestamp
        .clone();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Date", "Duration", "Exit"]);

    let start_idx = filtered.len().saturating_sub(10);
    for record in &filtered[start_idx..] {
        let display_ts = DateTime::parse_from_rfc3339(record.timestamp.as_str())
            .map(|dt| dt.format("%d.%m.%Y %H:%M").to_string())
            .unwrap_or_default();

        table.add_row(vec![
            Cell::new(display_ts),
            Cell::new(Playtime(record.duration_ms).to_string()),
            if record.exit_code == Some(0) {
                Cell::new("0").fg(Color::Green)
            } else {
                Cell::new(format!("{:?}", record.exit_code)).fg(Color::Red)
            },
        ]);
    }

    println!(
        "{} History for {} (Last 10)\n{table}",
        LOOKING_GLASS,
        style(&name).yellow().bold()
    );
    println!(
        "Total: {} | First: {} | Last: {}",
        style(Playtime(total_ms)).green(),
        style(format_ts(&first)),
        style(format_ts(&last))
    );
    Ok(())
}

fn show_summary(sort: SortBy, history_path: &Path) -> Result<()> {
    let all_records = load_records_jsonl(history_path)?;
    if all_records.is_empty() {
        println!("No records found.");
        return Ok(());
    }

    let mut aggregation: HashMap<CompactString, ProgramStats> = HashMap::new();

    for record in all_records {
        aggregation
            .entry(record.name)
            .and_modify(|entry| {
                entry.count += 1;
                entry.total_time_ms += record.duration_ms;
                if record.timestamp < entry.first_played {
                    entry.first_played = record.timestamp.clone();
                }
                if record.timestamp > entry.last_played {
                    entry.last_played = record.timestamp.clone();
                }
            })
            .or_insert_with(|| ProgramStats {
                count: 1,
                total_time_ms: record.duration_ms,
                first_played: record.timestamp.clone(),
                last_played: record.timestamp,
            });
    }

    let mut stats_vec: Vec<_> = aggregation.iter().collect();
    match sort {
        SortBy::Name => stats_vec.sort_by_key(|a| a.0.to_lowercase()),
        SortBy::Time => stats_vec.sort_by_key(|b| std::cmp::Reverse(b.1.total_time_ms)),
        SortBy::Sessions => stats_vec.sort_by_key(|b| std::cmp::Reverse(b.1.count)),
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_header(vec![
            "Program",
            "Sessions",
            "Total Time",
            "Avg Time",
            "Last Played",
        ]);

    for (name, stats) in stats_vec {
        let avg = stats.total_time_ms / stats.count;
        table.add_row(vec![
            Cell::new(name)
                .fg(Color::Yellow)
                .add_attribute(Attribute::Bold),
            Cell::new(stats.count),
            Cell::new(Playtime(stats.total_time_ms).to_string()),
            Cell::new(Playtime(avg).to_string()).fg(Color::Cyan),
            Cell::new(format_ts(&stats.last_played)),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (history_path, alias_path) = get_data_paths()?;

    match cli.command {
        Commands::Alias { action } => manage_alias(action, &alias_path),
        Commands::Run {
            name,
            command_parts,
        } => run_command(name, command_parts, &history_path, &alias_path),
        Commands::Info { name } => show_info(name, &history_path),
        Commands::Summary { sort } => show_summary(sort, &history_path),
    }
}
