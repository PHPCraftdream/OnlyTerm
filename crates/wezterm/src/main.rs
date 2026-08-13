#![warn(clippy::undocumented_unsafe_blocks)]
#[global_allocator]
static GLOBAL_ALLOC: sefer_alloc::SeferAlloc = sefer_alloc::SeferAlloc::new();

use anyhow::{anyhow, Context};
use clap::builder::ValueParser;
use clap::{Parser, ValueEnum, ValueHint};
use clap_complete::{generate as generate_completion, shells, Generator as CompletionGenerator};
use config::{wezterm_version, ConfigHandle};
use mux::Mux;
use std::ffi::OsString;
use umask::UmaskSaver;
use wezterm_gui_subcommands::*;

mod asciicast;
mod cli;
mod imgcat;
mod setcwd;

use imgcat::ImgCatCommand;
use setcwd::SetCwdCommand;

//    let message = "; ❤ 😍🤢\n\x1b[91;mw00t\n\x1b[37;104;m bleet\x1b[0;m.";

#[derive(Debug, Parser)]
#[command(
    about = "OnlyTerm - Terminal Emulator (fork of WezTerm)\nhttp://github.com/wezterm/wezterm",
    version = wezterm_version()
)]
pub struct Opt {
    /// Skip loading the wezterm config (ktav) file
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Specify the configuration file to use, overrides the normal
    /// configuration file resolution
    #[arg(
        long,
        value_parser,
        conflicts_with = "skip_config",
        value_hint=ValueHint::FilePath
    )]
    config_file: Option<OsString>,

    /// Override specific configuration values
    #[arg(
        long = "config",
        name = "name=value",
        value_parser=ValueParser::new(name_equals_value),
        number_of_values = 1)]
    config_override: Vec<(String, String)>,

    #[command(subcommand)]
    cmd: Option<SubCommand>,
}

#[derive(Debug, Clone, ValueEnum)]
// `PowerShell` ending in "Shell" trips clippy::enum_variant_names, but this is a
// clap `ValueEnum`: the variant name is serialized (kebab-cased) into the
// `--shell` CLI argument's accepted values. Renaming it would change the
// user-facing `wezterm shell-completion --shell <name>` value, which is a
// breaking CLI change, not just a naming cleanup.
#[allow(clippy::enum_variant_names)]
enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
    Fig,
}

impl CompletionGenerator for Shell {
    fn file_name(&self, name: &str) -> String {
        match self {
            Shell::Bash => shells::Bash.file_name(name),
            Shell::Elvish => shells::Elvish.file_name(name),
            Shell::Fish => shells::Fish.file_name(name),
            Shell::PowerShell => shells::PowerShell.file_name(name),
            Shell::Zsh => shells::Zsh.file_name(name),
            Shell::Fig => clap_complete_fig::Fig.file_name(name),
        }
    }

    fn generate(&self, cmd: &clap::Command, buf: &mut dyn std::io::Write) {
        match self {
            Shell::Bash => shells::Bash.generate(cmd, buf),
            Shell::Elvish => shells::Elvish.generate(cmd, buf),
            Shell::Fish => shells::Fish.generate(cmd, buf),
            Shell::PowerShell => shells::PowerShell.generate(cmd, buf),
            Shell::Zsh => shells::Zsh.generate(cmd, buf),
            Shell::Fig => clap_complete_fig::Fig.generate(cmd, buf),
        }
    }
}

#[derive(Debug, Parser, Clone)]
enum SubCommand {
    #[command(
        name = "start",
        about = "Start the GUI, optionally running an alternative program [aliases: -e]"
    )]
    Start(StartCommand),

    /// Start the GUI in blocking mode. You shouldn't see this, but you
    /// may see it in shell completions because of this open clap issue:
    /// <https://github.com/clap-rs/clap/issues/1335>
    #[command(short_flag_alias = 'e', hide = true)]
    BlockingStart(StartCommand),

    #[command(name = "serial", about = "Open a serial port")]
    Serial(SerialCommand),

    #[command(name = "connect", about = "Connect to OnlyTerm multiplexer")]
    Connect(ConnectCommand),

    #[command(name = "ls-fonts", about = "Display information about fonts")]
    LsFonts(LsFontsCommand),

    #[command(name = "show-keys", about = "Show key assignments")]
    ShowKeys(ShowKeysCommand),

    #[command(name = "cli", about = "Interact with experimental mux server")]
    Cli(cli::CliCommand),

    #[command(name = "imgcat", about = "Output an image to the terminal")]
    ImageCat(ImgCatCommand),

    #[command(
        name = "set-working-directory",
        about = "Advise the terminal of the current working directory by \
                 emitting an OSC 7 escape sequence"
    )]
    SetCwd(SetCwdCommand),

    #[command(name = "record", about = "Record a terminal session as an asciicast")]
    Record(asciicast::RecordCommand),

    #[command(name = "replay", about = "Replay an asciicast terminal session")]
    Replay(asciicast::PlayCommand),

    /// Generate shell completion information
    #[command(name = "shell-completion")]
    ShellCompletion {
        /// Which shell to generate for
        #[arg(long, value_parser)]
        shell: Shell,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum, Default)]
enum TmuxPassthru {
    Disable,
    Enable,
    #[default]
    Detect,
}

impl TmuxPassthru {
    fn is_tmux() -> bool {
        std::env::var_os("TMUX").is_some()
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Enable => true,
            Self::Detect => Self::is_tmux(),
            Self::Disable => false,
        }
    }

    fn encode(&self, content: String) -> String {
        if self.enabled() {
            let mut result = "\u{1b}Ptmux;".to_string();
            for c in content.chars() {
                if c == '\u{1b}' {
                    // Quote the escape by doubling it up
                    result.push(c);
                }
                result.push(c);
            }
            result.push_str("\u{1b}\\");
            result
        } else {
            content
        }
    }
}

fn terminate_with_error_message(err: &str) -> ! {
    log::error!("{}; terminating", err);
    std::process::exit(1);
}

fn terminate_with_error(err: anyhow::Error) -> ! {
    terminate_with_error_message(&format!("{:#}", err));
}

fn main() {
    // `config::designate_this_as_the_main_thread()` used to be called here
    // to initialize the rhai event-callback state for any rhai call sites
    // in this binary. With no remaining call sites into the rhai
    // event-callback bridge, that designation is no longer needed here.
    config::assign_error_callback(mux::connui::show_configuration_error_message);
    if let Err(e) = run() {
        terminate_with_error(e);
    }
    Mux::shutdown();
}

fn init_config(opts: &Opt) -> anyhow::Result<ConfigHandle> {
    config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )
    .context("config::common_init")?;
    let config = config::configuration();
    config.update_ulimit()?;
    if let Some(value) = &config.default_ssh_auth_sock {
        std::env::set_var("SSH_AUTH_SOCK", value);
    }
    Ok(config)
}

fn run() -> anyhow::Result<()> {
    env_bootstrap::bootstrap();

    let _saver = UmaskSaver::new();

    let opts = Opt::parse();

    match opts
        .cmd
        .as_ref()
        .cloned()
        .unwrap_or_else(|| SubCommand::Start(StartCommand::default()))
    {
        SubCommand::Start(_)
        | SubCommand::BlockingStart(_)
        | SubCommand::LsFonts(_)
        | SubCommand::ShowKeys(_)
        | SubCommand::Serial(_)
        | SubCommand::Connect(_) => delegate_to_gui(),
        SubCommand::ImageCat(cmd) => cmd.run(),
        SubCommand::SetCwd(cmd) => cmd.run(),
        SubCommand::Cli(cli) => cli::run_cli(&opts, cli),
        SubCommand::Record(cmd) => cmd.run(init_config(&opts)?),
        SubCommand::Replay(cmd) => cmd.run(),
        SubCommand::ShellCompletion { shell } => {
            use clap::CommandFactory;
            let mut cmd = Opt::command();
            let name = cmd.get_name().to_string();
            generate_completion(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
    }
}

fn delegate_to_gui() -> anyhow::Result<()> {
    use std::process::Command;

    let exe_name = if cfg!(windows) {
        "onlyterm-gui.exe"
    } else {
        "onlyterm-gui"
    };

    let exe = std::env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow!("exe has no parent dir!?"))?
        .join(exe_name);

    let mut cmd = Command::new(exe);
    if cfg!(windows) {
        cmd.arg("--attach-parent-console");
    }

    cmd.args(std::env::args_os().skip(1));

    let mut child = cmd.spawn()?;
    let status = child.wait()?;
    let code = status.code().unwrap_or(1);
    std::process::exit(code);
}
