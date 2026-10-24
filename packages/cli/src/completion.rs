//! `rsmix completion <bash|zsh>` (Remix's `remix completion`): a shell script that asks
//! `rsmix completion -- <index> <words...>` for candidates, so completion follows the CLI.

/// The shells `rsmix completion` writes scripts for.
pub const SHELLS: [&str; 2] = ["bash", "zsh"];

const ROOT_COMMANDS: [&str; 10] = ["assets", "build", "completion", "dev", "doctor", "help", "new", "routes", "test", "version"];

/// What the shell should offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completion {
    /// These words (already filtered by the word being completed).
    Values(Vec<String>),
    /// File names.
    Files,
    /// Nothing.
    None,
}

impl Completion {
    /// The protocol the scripts read: `mode:values` then one value per line, `mode:files`
    /// or `mode:none`.
    pub fn render(&self) -> String {
        match self {
            Completion::Values(values) => {
                let mut out = String::from("mode:values\n");
                for value in values {
                    out.push_str(value);
                    out.push('\n');
                }
                out
            }
            Completion::Files => "mode:files\n".into(),
            Completion::None => "mode:none\n".into(),
        }
    }
}

fn values(candidates: &[&str], current: &str, used: &[&str]) -> Completion {
    Completion::Values(candidates.iter().filter(|c| c.starts_with(current) && !used.contains(c)).map(|c| c.to_string()).collect())
}

/// Candidates for `words[index]`, where `words[0]` is the program name. As in Remix, every
/// list ends with the global flags not used yet (`-h`, `--help`, `--no-color`, and at the
/// top level `-v`, `--version`).
pub fn complete(words: &[String], index: usize) -> Completion {
    let current = words.get(index).map_or("", String::as_str);
    let all: Vec<&str> = words.iter().take(index).skip(1).map(String::as_str).collect();
    // `--no-color` is global: it may come before or after the command.
    let no_color = all.contains(&"--no-color");
    let tokens: Vec<&str> = all.into_iter().filter(|t| *t != "--no-color").collect();
    fn with_global<'a>(list: &[&'a str], no_color: bool, top: bool) -> Vec<&'a str> {
        let mut out = list.to_vec();
        out.extend(["-h", "--help"]);
        if !no_color {
            out.push("--no-color");
        }
        if top {
            out.extend(["-v", "--version"]);
        }
        out
    }
    let Some((&command, rest)) = tokens.split_first() else {
        return values(&with_global(&ROOT_COMMANDS, no_color, true), current, &[]);
    };
    if matches!(command, "-h" | "--help" | "-v" | "--version") || rest.iter().any(|t| matches!(*t, "-h" | "--help")) {
        return Completion::Values(Vec::new());
    }
    let flags = |list: &[&str]| values(&with_global(list, no_color, false), current, rest);
    match command {
        "help" if rest.is_empty() => flags(&ROOT_COMMANDS),
        "completion" if rest.is_empty() => flags(&SHELLS),
        "assets" => match rest {
            [] => flags(&["inspect"]),
            ["inspect"] if !current.starts_with('-') => Completion::Files,
            ["inspect"] => flags(&[]),
            _ => Completion::Values(Vec::new()),
        },
        "new" => {
            if rest.last() == Some(&"--app-name") {
                return Completion::None;
            }
            let has_target = rest.iter().enumerate().any(|(i, t)| !t.starts_with('-') && (i == 0 || rest[i - 1] != "--app-name"));
            if current.starts_with('-') {
                flags(&["--app-name", "--force"])
            } else if has_target {
                Completion::Values(Vec::new())
            } else {
                Completion::Files
            }
        }
        "doctor" => flags(&["--fix", "--json", "--strict", "--no-strict"]),
        "routes" => {
            // As Remix's `completeRoutes`: no flag that cannot combine with those given, and
            // nothing after an unknown token.
            if rest.iter().any(|t| !matches!(*t, "--json" | "--table" | "--no-headers")) {
                return Completion::Values(Vec::new());
            }
            let (json, table) = (rest.contains(&"--json"), rest.contains(&"--table"));
            let mut offered = Vec::new();
            if !json && !table {
                offered.extend(["--json", "--table"]);
            }
            if table {
                offered.push("--no-headers");
            }
            flags(&offered)
        }
        "dev" if !rest.contains(&"--") => flags(&["--"]),
        "version" | "build" => flags(&[]),
        _ => Completion::Values(Vec::new()),
    }
}

/// The completion script; one script serves bash and zsh, as in Remix.
pub fn script() -> &'static str {
    r#"###-begin-rsmix-completion-###
#
# rsmix command completion script
#
# Installation:
#   rsmix completion bash >> ~/.bashrc
#   rsmix completion zsh >> ~/.zshrc
#

if type complete &>/dev/null; then
  _rsmix_completion() {
    local words cword output mode si
    local lines

    if type _get_comp_words_by_ref &>/dev/null; then
      _get_comp_words_by_ref -n = -w words -i cword
    else
      cword="$COMP_CWORD"
      words=("${COMP_WORDS[@]}")
    fi

    output=$(rsmix completion -- "$cword" "${words[@]}" 2>/dev/null) || return 0
    si="$IFS"
    IFS=$'\n' lines=($output)
    IFS="$si"
    mode="${lines[0]}"

    if [[ "$mode" == "mode:values" ]]; then
      COMPREPLY=("${lines[@]:1}")
      return 0
    fi

    if [[ "$mode" == "mode:files" ]]; then
      COMPREPLY=($(compgen -f -- "${words[cword]}"))
      return 0
    fi

    COMPREPLY=()
  }

  complete -o default -F _rsmix_completion rsmix
elif type compdef &>/dev/null; then
  _rsmix_completion() {
    local output mode
    local -a lines values

    output=$(rsmix completion -- "$((CURRENT - 1))" "${words[@]}" 2>/dev/null) || return 0
    lines=("${(@f)output}")
    mode="${lines[1]}"

    if [[ "$mode" == "mode:values" ]]; then
      values=("${(@)lines[2,-1]}")
      if (( ${#values[@]} > 0 )); then
        compadd -- "${values[@]}"
      fi
      return 0
    fi

    if [[ "$mode" == "mode:files" ]]; then
      if autoload -U +X _files 2>/dev/null; then
        _files
      else
        compadd -f
      fi
      return 0
    fi

    return 0
  }

  compdef _rsmix_completion rsmix
fi
###-end-rsmix-completion-###
"#
}
