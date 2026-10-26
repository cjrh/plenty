# User directories

`import std.fs.dirs` finds the user's home directory and the per-user base
directories where a program keeps its configuration, caches, data, and state.
It is written in Plenty over [`std.env`](../env.md) and returns
[`Path`](../path.md) values.

| Function | Variable | Otherwise |
|---|---|---|
| `home_dir()` | `HOME` | `Nothing` |
| `config_dir()` | `XDG_CONFIG_HOME` | `$HOME/.config` |
| `cache_dir()` | `XDG_CACHE_HOME` | `$HOME/.cache` |
| `data_dir()` | `XDG_DATA_HOME` | `$HOME/.local/share` |
| `state_dir()` | `XDG_STATE_HOME` | `$HOME/.local/state` |

Each returns `Result[Option[Path], IoError]`. The rules follow the
[XDG Base Directory specification](https://specifications.freedesktop.org/basedir-spec/latest/)
and the Rust `dirs` crate on Linux:

- An empty variable counts as unset.
- An `XDG_*` value is used only when it is absolute. The specification calls a
  relative value invalid, so it is ignored and the default below `HOME` is used.
- `HOME` is used as set, without requiring an absolute path.
- With no usable `XDG_*` value and no `HOME`, the result is `Nothing`.

```plenty
import std.fs.dirs
from std.path import from_text

def main() -> Result[(), IoError]:
    match std.fs.dirs.config_dir()?:
        case Some(config):
            settings = config.join(&from_text("myapp/settings.toml")?)?
            print(settings.display()?)?
        case Nothing:
            print("no configuration directory")?
    Ok(())
```

Paths keep the variable's exact bytes, so a directory name that is not UTF-8
round-trips. Results are not cached: each call reads the environment again.
Nothing is created or checked; a returned directory may not exist yet. Create
it with the [rooted helpers](helpers.md) when the program first writes there.
Allocation failure is `IoError.Data(DataError.Allocation(...))`.

## Limits

- x86-64 Linux GNU only, like [`std.env`](../env.md); elsewhere each call
  reports `IoError.Unsupported`.
- No password-database fallback: when `HOME` is unset, `dirs` consults
  `getpwuid_r`, but `home_dir()` returns `Nothing`.
- No runtime, executable, or system-wide (`XDG_CONFIG_DIRS`, `XDG_DATA_DIRS`)
  directories.

## Acknowledgements

- Follows the [XDG Base Directory specification](https://specifications.freedesktop.org/basedir-spec/latest/) as the Rust [`dirs`](https://crates.io/crates/dirs) crate applies it on Linux.
