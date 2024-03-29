#![forbid(unused_must_use)]
#![forbid(unsafe_code)]

pub(crate) mod blueprint;
mod osrelease;

use std::process::Command;
use std::{io::Read, path::Path};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cap_std_ext::cap_std::{self, fs::Dir};
use clap::{Parser, Subcommand};
use fn_error_context::context;

use crate::osrelease::verify_osrelease;

pub(crate) const USR_TMPFILES: &str = "usr/lib/tmpfiles.d";

/// An opinionated tool to process declarative input files as
/// part of a container build for configuring Linux operating systems.
#[derive(Debug, Parser, PartialEq, Eq)]
#[clap(name = "osbuild-cfg")]
#[clap(rename_all = "kebab-case", version)]
pub(crate) struct Opt {
    #[clap(long)]
    pub(crate) dry_run_output: Option<Utf8PathBuf>,

    #[clap(subcommand)]
    cmd: Cmd,
}

/// An opinionated tool to process declarative input files as
/// part of a container build for configuring Linux operating systems.
#[derive(Debug, Subcommand, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
pub(crate) enum Cmd {
    Blueprint(BlueprintOpts),
}

#[derive(Debug, Parser, PartialEq, Eq)]
#[clap(rename_all = "kebab-case", version)]
pub(crate) struct BlueprintOpts {
    /// Path to the blueprint.
    pub(crate) path: Utf8PathBuf,
}

/// A command to execute
pub(crate) struct ExecuteCommand(Vec<String>);

/// The rendered output
pub(crate) struct Rendered<'d> {
    /// Generated bash code
    pub(crate) exec: Vec<ExecuteCommand>,
    pub(crate) filesystem: &'d Dir,
}

impl<'d> Rendered<'d> {
    #[context("Creating new rendered data")]
    pub(crate) fn new(filesystem: &'d Dir) -> Result<Self> {
        Ok(Self {
            exec: Vec::new(),
            filesystem,
        })
    }
}

trait Render {
    fn render(&self, srcroot: &Dir, out: &mut Rendered) -> Result<bool>;
}

#[context("Determining self-consume status")]
fn should_consume(self_path: &Path) -> Result<bool> {
    if let Some(self_path) = self_path.to_str() {
        if self_path.contains("/target/") {
            tracing::debug!("Running a cargo binary");
            return Ok(false);
        }
    }
    if std::env::var_os("container").is_none() {
        tracing::debug!("$container is not set");
        return Ok(false);
    }
    if !rustix::process::getuid().is_root() {
        tracing::debug!("Not running as root");
        return Ok(false);
    }

    Ok(true)
}

/// Convenience helper to accept `-` as standard input
pub(crate) fn reader_or_stdin(p: &Utf8Path) -> Result<impl Read> {
    let r = match p.as_str() {
        "-" => either::Either::Left(std::io::stdin()),
        p => {
            let f = std::fs::File::open(p)
                .map(std::io::BufReader::new)
                .with_context(|| format!("Reading {}", p))?;
            either::Either::Right(f)
        }
    };
    Ok(r)
}

fn run() -> Result<()> {
    // Don't include timestamps and such because they're not really useful and
    // too verbose, and plus several log targets such as journald will already
    // include timestamps.
    let format = tracing_subscriber::fmt::format()
        .without_time()
        .with_target(false)
        .compact();
    // Log to stderr by default
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .event_format(format)
        .with_writer(std::io::stderr)
        .init();
    tracing::trace!("starting");

    let self_path = std::fs::read_link("/proc/self/exe").context("Reading /proc/self/exe")?;
    let should_consume = should_consume(&self_path)?;
    tracing::debug!("should_consume={should_consume}");

    let opt = Opt::parse();

    // Set up our source and target directories
    let root = &Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    let target_dir = opt
        .dry_run_output
        .as_ref()
        .map(|p| {
            if !p.try_exists()? {
                std::fs::create_dir(p).with_context(|| format!("{p}"))?;
            }
            Dir::open_ambient_dir(p, cap_std::ambient_authority())
                .with_context(|| format!("Opening {p}"))
        })
        .transpose()?;
    let target_dir = target_dir.as_ref().unwrap_or(root);
    let dry_run = opt.dry_run_output.is_some();
    if dry_run {
        if target_dir.entries()?.next().is_some() {
            anyhow::bail!("Refusing to operate on non-empty directory");
        }
    } else {
        if !rustix::process::getuid().is_root() {
            anyhow::bail!("This program must be run as root (in non --dry-run mode)");
        }
        verify_osrelease()?;
    }
    let mut rendered = Rendered::new(target_dir)?;

    match opt.cmd {
        Cmd::Blueprint(opts) => {
            let blueprint_path = &opts.path;
            println!("Processing blueprint: {blueprint_path}");
            let mut reader = reader_or_stdin(&blueprint_path)?;
            let mut buf = String::new();
            reader
                .read_to_string(&mut buf)
                .with_context(|| format!("Reading {}", blueprint_path))?;
            let blueprint: blueprint::Blueprint =
                toml::from_str(&buf).with_context(|| format!("Parsing {}", blueprint_path))?;
            blueprint.render(root, &mut rendered)?;
            if should_consume {
                println!("Removing {}", blueprint_path);
                std::fs::remove_file(&blueprint_path)?;
            }
        }
    }

    if dry_run {
        println!("Dry-run mode enabled");
    }

    if rendered.exec.is_empty() {
        println!("(No commands to execute)");
    }
    for cmd in rendered.exec {
        println!("+ {:?}", cmd.0);
        if dry_run {
            continue;
        }
        let mut cmd = cmd.0.into_iter();
        let prog = cmd.next().unwrap();

        let st = Command::new(&prog).args(cmd).status()?;
        if !st.success() {
            anyhow::bail!("Failed to execute {prog}");
        }
    }

    if let Some(td) = opt.dry_run_output.as_deref() {
        println!("Generated dry-run output in: {td}");
        let st = Command::new("tree").current_dir(td).status()?;
        if !st.success() {
            anyhow::bail!("Failed to execute tree: {st:?}");
        }
    } else {
        if should_consume {
            println!("Removing self: {self_path:?}");
            std::fs::remove_file(&self_path).with_context(|| format!("Removing {self_path:?}"))?;
        }
        println!("Execution complete.");
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}
