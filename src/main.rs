#![forbid(unused_must_use)]
#![forbid(unsafe_code)]

pub(crate) mod blueprint;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use cap_std_ext::cap_std::{self, fs::Dir};
use clap::Parser;
use fn_error_context::context;

pub(crate) const USR_TMPFILES: &str = "usr/lib/tmpfiles.d";

/// An opinionated tool to process declarative input files as
/// part of a container build for configuring Linux operating systems.
#[derive(Debug, Parser, PartialEq, Eq)]
#[clap(name = "osbuild-cfg")]
#[clap(rename_all = "kebab-case", version)]
pub(crate) enum Opt {
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
pub(crate) struct Rendered {
    /// Generated bash code
    pub(crate) exec: Vec<ExecuteCommand>,
    pub(crate) filesystem: cap_std_ext::cap_tempfile::TempDir,
}

impl Rendered {
    #[context("Creating new rendered data")]
    pub(crate) fn new() -> Result<Self> {
        let filesystem = cap_std_ext::cap_tempfile::TempDir::new(cap_std::ambient_authority())?;
        Ok(Self {
            exec: Vec::new(),
            filesystem,
        })
    }
}

trait Render {
    fn render(&self, srcroot: &Dir, out: &mut Rendered) -> Result<bool>;
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

    let opt = Opt::parse();
    let mut rendered = Rendered::new()?;
    let root = &Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    match opt {
        Opt::Blueprint(opts) => {
            let blueprint = std::fs::read_to_string(&opts.path)
                .with_context(|| format!("Reading {}", opts.path))?;
            let blueprint: blueprint::Blueprint =
                toml::from_str(&blueprint).with_context(|| format!("Parsing {}", opts.path))?;
            blueprint.render(root, &mut rendered)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        tracing::error!("{:#}", e);
        std::process::exit(1);
    }
}
