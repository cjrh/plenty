use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(directory: &Path, body: &str) -> PathBuf {
    let source = directory.join("progress.plenty");
    let executable = directory.join("progress");
    std::fs::write(
        &source,
        format!(
            r#"
import std.cli.progress as progress
import std.report as report
import std.time as time
import std.io as io
from std.cli.progress import ProgressError
from std.report import FormatError
def main() -> Result[(), ProgressError | FormatError | IoError]:
{body}
    Ok(())
"#
        ),
    )
    .unwrap();
    plenty::compile_file_to_executable(&source, &executable, None)
        .unwrap_or_else(|error| panic!("{error}"));
    executable
}

fn run(body: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(compile(directory.path(), body))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn deterministic_render_metrics_and_redirected_streams() {
    run(
        r#"    mut bar = progress.bar_at(100u64, &time.instant_at(0u64))
    bar.set_position_at(25u64, &time.instant_at(2000000000u64))?
    now = time.instant_at(2000000000u64)
    mut output = report.string_formatter(128, 0)?
    bar.render(&now, &mut output)?
    print(report.finish(&output)?)?
    print(bar.position(), bar.total(), bar.rate(&now).unwrap(), bar.eta(&now).unwrap())?
    print(io.stdin().is_terminal(), io.stdout().is_terminal(), io.stderr().is_terminal())?
    bar.finish_at(&now)?
    bar.finish_at(&now)?
    print(bar.position(), bar.is_finished())?"#,
        "[##--------] 25/100 12/s ETA 6s\n25 Option[u64].Some(100) 12.5 6.0\nFalse False False\n25 True\n",
    );
}

#[test]
fn zero_unknown_extreme_and_backwards_time_are_bounded() {
    run(
        r#"    now = time.instant_at(1u64)
    mut empty = progress.bar_at(0u64, &now)
    mut output = report.string_formatter(128, 0)?
    empty.render(&now, &mut output)?
    print(report.finish(&output)?)?
    mut spin = progress.spinner_at(&now)
    spin.set_position_at(u64.MAX, &time.instant_at(2u64))?
    mut spin_output = report.string_formatter(128, 0)?
    spin.render(&time.instant_at(2u64), &mut spin_output)?
    print(report.finish(&spin_output)?)?
    print(spin.rate(&time.instant_at(0u64)).is_none(), spin.eta(&now).is_none())?
    print(spin.advance().is_err(), spin.position() == u64.MAX)?
    mut full = progress.bar_at(u64.MAX, &now)
    full.set_position_at(u64.MAX, &now)?
    mut full_output = report.string_formatter(128, 0)?
    full.render(&now, &mut full_output)?
    print(report.finish(&full_output)?)?"#,
        "[##########] 0/0 --/s ETA 0s\n| 18E 18E/s ETA --\nTrue True\nTrue True\n[##########] 18E/18E --/s ETA 0s\n",
    );
}

#[test]
fn invalid_positions_are_transactional_and_render_preserves_errors() {
    run(
        r#"    mut bar = progress.bar(10u64)
    bar.advance(4u64)?
    print(bar.set_position(3u64).is_err(), bar.set_position(11u64).is_err(), bar.position())?
    mut output = report.string_formatter(0, 0)?
    print(bar.render(&time.monotonic_now(), &mut output).is_err())?
    bar.finish()?
    print(bar.advance().is_err(), bar.tick().is_err(), bar.position())?"#,
        "True True 4\nTrue\nTrue True 4\n",
    );
}

#[cfg(unix)]
#[test]
fn terminal_redraw_throttle_finish_and_output_failure() {
    let directory = tempfile::tempdir().unwrap();
    let executable = compile(
        directory.path(),
        r#"    mut bar = progress.bar_at(10u64, &time.instant_at(0u64))
    match bar.set_position_at(1u64, &time.instant_at(0u64)):
        case Err(error):
            match error:
                case ProgressError.Output(_):
                    print("output failure", bar.is_finished())?
                    return Ok(())
                case _ as other:
                    return Err(other)
        case Ok(_):
            pass
    bar.set_position_at(2u64, &time.instant_at(99999999u64))?
    bar.set_position_at(3u64, &time.instant_at(100000000u64))?
    bar.set_position_at(4u64, &time.instant_at(0u64))?
    bar.finish_at(&time.instant_at(100000001u64))?
    bar.finish()?
    mut spin = progress.spinner_at(&time.instant_at(0u64))
    spin.set_position_at(0u64, &time.instant_at(0u64))?
    spin.set_position_at(0u64, &time.instant_at(100000000u64))?
    spin.finish_at(&time.instant_at(100000001u64))?"#,
    );
    let output = Command::new("python3").args(["-c", r#"
import errno, os, pty, subprocess, sys, termios
master, slave = pty.openpty()
try:
    result = subprocess.run([sys.argv[1]], stderr=slave, stdout=subprocess.PIPE, timeout=20)
    os.close(slave)
    data = b''
    while True:
        try:
            chunk = os.read(master, 4096)
        except OSError as error:
            if error.errno == errno.EIO: break
            raise
        if not chunk: break
        data += chunk
    assert result.returncode == 0, (result, data)
    assert result.stdout == b'', result.stdout
    frames = data.split(b'\r\x1b[2K')[1:]
    assert len(frames) == 6, data
    assert b'1/10' in frames[0] and b'3/10' in frames[1] and b'4/10' in frames[2], data
    assert frames[3].startswith(b'| ') and frames[4].startswith(b'/ ') and frames[5].startswith(b'done '), data
    assert data.count(b'\n') == 2, data
finally:
    os.close(master)
master, slave = pty.openpty()
try:
    # A stopped, nonblocking terminal reports EAGAIN without racing a reader.
    termios.tcflow(slave, termios.TCOOFF)
    os.set_blocking(slave, False)
    while True:
        try:
            os.write(slave, b'x')
        except BlockingIOError:
            break
    result = subprocess.run([sys.argv[1]], stderr=slave, stdout=subprocess.PIPE, timeout=20)
    assert result.returncode == 0 and result.stdout == b'output failure True\n', result
finally:
    os.close(slave)
    os.close(master)
"#]).arg(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
