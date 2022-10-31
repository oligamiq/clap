//! Generates [Nushell](https://github.com/nushell/nushell) completions for [`clap`](https://github.com/clap-rs/clap) based CLIs

use clap::{Arg, Command};
use clap_complete::Generator;

/// Generate Nushell complete file
pub struct Nushell;

enum Argument {
    Short(Vec<char>),
    Long(Vec<String>),
    ShortAndLong(Vec<char>, Vec<String>),
    Positional(String, bool),
}

struct ArgumentLine {
    arg: Argument,
    takes_values: bool,
    help: Option<String>,
}

impl ArgumentLine {
    fn append_type_and_help(&self, s: &mut String) {
        if self.takes_values {
            s.push_str(": string");
        }

        if let Some(help) = &self.help {
            s.push_str(format!("\t# {}", help).as_str());
        }

        s.push('\n');
    }
}

impl From<&Arg> for ArgumentLine {
    fn from(arg: &Arg) -> Self {
        let takes_values = arg
            .get_num_args()
            .map(|v| v.takes_values())
            .unwrap_or(false);

        let help = arg.get_help().map(|s| s.to_string());

        if arg.is_positional() {
            let id = arg.get_id().to_string();
            let required = arg.is_required_set();
            let arg = Argument::Positional(id, required);

            return Self {
                arg,
                takes_values,
                help,
            };
        }

        let shorts = arg.get_short_and_visible_aliases();
        let longs = arg.get_long_and_visible_aliases();

        match shorts {
            Some(shorts) => match longs {
                Some(longs) => Self {
                    arg: Argument::ShortAndLong(
                        shorts,
                        longs.iter().map(|s| s.to_string()).collect(),
                    ),
                    takes_values,
                    help,
                },
                None => Self {
                    arg: Argument::Short(shorts),
                    takes_values,
                    help,
                },
            },
            None => match longs {
                Some(long) => Self {
                    arg: Argument::Long(long.iter().map(|s| s.to_string()).collect()),
                    takes_values,
                    help,
                },
                None => unreachable!("No short or long option found"),
            },
        }
    }
}

impl ToString for ArgumentLine {
    fn to_string(&self) -> String {
        let mut s = String::new();

        match &self.arg {
            Argument::Short(shorts) => {
                for short in shorts {
                    s.push_str(format!("    -{}", short).as_str());
                    self.append_type_and_help(&mut s);
                }
            }
            Argument::Long(longs) => {
                for long in longs {
                    s.push_str(format!("    --{}", long).as_str());
                    self.append_type_and_help(&mut s);
                }
            }
            Argument::ShortAndLong(shorts, longs) => {
                s.push_str(
                    format!(
                        "    --{}(-{})",
                        longs.first().expect("At least one long option expected"),
                        shorts.first().expect("At lease one short option expected")
                    )
                    .as_str(),
                );
                self.append_type_and_help(&mut s);

                // long alias
                for long in longs.iter().skip(1) {
                    s.push_str(format!("    --{}", long).as_str());
                    self.append_type_and_help(&mut s);
                }

                // short alias
                for short in shorts.iter().skip(1) {
                    s.push_str(format!("    -{}", short).as_str());
                    self.append_type_and_help(&mut s);
                }
            }
            Argument::Positional(positional, required) => {
                s.push_str(format!("    {}", positional).as_str());

                if !*required {
                    s.push('?');
                }

                self.append_type_and_help(&mut s);
            }
        }

        s
    }
}

impl Generator for Nushell {
    fn file_name(&self, name: &str) -> String {
        format!("{}.nu", name)
    }

    fn generate(&self, cmd: &Command, buf: &mut dyn std::io::Write) {
        let mut completions = String::new();

        completions.push_str("module completions {\n\n");

        generate_completion(&mut completions, cmd, false);

        for sub in cmd.get_subcommands() {
            generate_completion(&mut completions, sub, true);
        }

        completions.push_str("}\n\n");
        completions.push_str("use completions *\n");

        buf.write_all(completions.as_bytes())
            .expect("Failed to write to generated file")
    }
}

fn generate_completion(completions: &mut String, cmd: &Command, is_subcommand: bool) {
    if let Some(about) = cmd.get_about() {
        completions.push_str(format!("  # {}\n", about).as_str());
    }

    let bin_name = cmd.get_bin_name().expect("Failed to get bin name");

    let name = if is_subcommand {
        format!(r#""{}""#, bin_name)
    } else {
        bin_name.into()
    };

    completions.push_str(format!("  export extern {} [\n", name).as_str());

    let s: String = cmd
        .get_arguments()
        .map(|arg| ArgumentLine::from(arg).to_string())
        .collect();

    completions.push_str(&s);
    completions.push_str("  ]\n\n");

    // For sub-subcommands
    if is_subcommand {
        for sub in cmd.get_subcommands() {
            generate_completion(completions, sub, true);
        }
    }
}
