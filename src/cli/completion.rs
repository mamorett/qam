use clap::Command;
use clap_complete::{Shell, generate};
use std::io;

pub fn execute(shell: Shell, cmd: &mut Command) {
    let bin_name = cmd.get_name().to_string();
    generate(shell, cmd, bin_name, &mut io::stdout());
}
