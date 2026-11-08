//! Runtime-backed text readers consume the selected UTF-8 range, with no owner-header ABI.
mod support;
use support::run_file_stdout;

#[test]
fn crypto_hashes_exact_view_bytes_and_derive_context() {
    let source = r#"import std.crypto
import std.bytes
from std.crypto import Algorithm

def main() -> Result[(), IoError | AllocError | std.crypto.CryptoError]:
    source = "outsideé\0☃tail"
    view = &source[7:10]
    mut selected = std.crypto.digest(Algorithm.Sha256)?
    selected.update_text(view)?
    mut expected = std.crypto.digest(Algorithm.Sha256)?
    expected.update_text("é\0☃")?
    a = selected.finish()?
    b = expected.finish()?
    print(std.crypto.equal(&a, &b))?
    context = "!example context!"
    first = std.crypto.blake3_derive_key(&context[1:16], b"secret")?
    second = std.crypto.blake3_derive_key("example context", b"secret")?
    print(std.crypto.equal(&first, &second))?
    bytes = std.bytes.from_text("!é\0☃!")?
    print(bytes.matches_text(1, view), bytes.matches_text(2, view))?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True\nTrue\nTrue False");
}

#[test]
fn sqlite_selected_sql_and_copied_text_keep_embedded_nul() {
    let source = r#"import std.db.sqlite

def main() -> Result[(), IoError | AllocError | std.db.sqlite.DbError]:
    mut db = std.db.sqlite.memory(4096)?
    sql = "\0SELECT :value\0"
    mut query = db.prepare(&sql[1:14])?
    name = "!:value!"
    index = query.parameter_index(&name[1:7])?
    text = "!é\0☃!"
    query.bind_text(index, &text[1:4])?
    print(query.step()?)?
    print(query.text(0)? == "é\0☃")?
    empty = "!"
    query.reset()
    query.bind_text(index, &empty[1:1])?
    query.step()?
    print(query.text(0)? == "")?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True\nTrue\nTrue");
}

#[test]
fn parsers_and_path_conversion_use_only_selected_range() {
    let source = r#"import std.net
import std.path

def main() -> Result[(), IoError | AllocError | std.net.NetError]:
    source = "\0127.0.0.1:80\0"
    address = std.net.parse_address(&source[1:13])?
    print(address.port)?
    ip = std.net.parse_ip(&source[1:10])?
    print(std.net.format_ip(&ip)? == "127.0.0.1")?
    path = "\0é/☃\0"
    native = std.path.from_text(&path[1:4])?
    print(native.to_text()? == "é/☃")?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "80\nTrue\nTrue");
}

#[test]
fn byte_view_decoding_validates_selected_range_without_buffer_copy() {
    let source = r#"import std.bytes

def main() -> Result[(), IoError | AllocError | std.bytes.BytesError]:
    source = b"\xff\xc3\xa9\0\xe2\x98\x83\xff"
    print(std.bytes.to_text(&source[1:7])? == "é\0☃")?
    print(std.bytes.to_text(&source[0:0])? == "")?
    match std.bytes.to_text(&source[:1]):
        case Err(std.bytes.BytesError.InvalidUtf8):
            print(True)?
        case _:
            print(False)?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True\nTrue\nTrue");
}

#[test]
fn arbitrary_bytes_cannot_cross_utf8_native_text_boundary() {
    let source = r#"import std.crypto

def main() -> ():
    mut state = std.crypto.digest(std.crypto.Algorithm.Sha256).unwrap()
    state.update_text(b"\xff").unwrap()
"#;
    let error = support::check_file_error(source);
    assert!(error.contains("strview"), "{error}");
}

#[test]
fn retained_process_arguments_outlive_their_text_view_owners() {
    let source = r#"import std.process
import std.io
import std.bytes

def configured() -> Result[std.process.Command, IoError]:
    program = "\0printf\0"
    mut command = std.process.command(&program[1:7])?
    format = "\0%s\0"
    command.arg(&format[1:3])?
    argument = "\0é☃\0"
    command.arg(&argument[1:3])?
    command.stdout(std.process.Stdio.Piped)
    Ok(command)

def main() -> Result[(), IoError | AllocError | std.io.StreamError | std.bytes.BytesError]:
    command = configured()?
    mut child = command.spawn()?
    result = child.output(1000)?
    print(result.stdout.to_text()? == "é☃", result.status.success())?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True True");
}

#[test]
fn retained_tls_pem_recipes_and_names_outlive_their_views() {
    let fixtures = support::tls_fixtures();
    let source = r#"import std.tls

def server() -> Result[std.tls.ServerBuilder, IoError | AllocError | std.tls.TlsError]:
    cert = ("\0" + read_text("FIXTURES/leaf.pem")?)?
    key = ("\0" + read_text("FIXTURES/leaf.key")?)?
    std.tls.server_builder(&cert[1:], &key[1:])

def client() -> Result[std.tls.ClientConfig, IoError | AllocError | std.tls.TlsError]:
    pem = ("\0" + read_text("FIXTURES/ca.pem")?)?
    roots = std.tls.roots_from_pem(&pem[1:])?
    mut builder = std.tls.client_builder(&roots)?
    protocol = "\0h2\0"
    builder.application_protocol(&protocol[1:3])?
    builder.build()

def session(config: &std.tls.ClientConfig) -> Result[std.tls.Session, std.tls.TlsError]:
    name = "\0localhost\0"
    std.tls.client_session(config, &name[1:10])

def main() -> Result[(), IoError | AllocError | std.tls.TlsError]:
    builder = server()?
    drop(builder.build()?)
    config = client()?
    drop(session(&config)?)
    print(True)?
    Ok(())
"#
    .replace("FIXTURES", &fixtures);
    assert_eq!(run_file_stdout(&source), "True");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn readonly_hashing_and_ipv4_parsing_need_no_owned_text_copy() {
    let source = r#"import std.crypto
import std.net

def main() -> Result[(), IoError | AllocError | std.crypto.CryptoError | std.net.NetError]:
    mut digest = std.crypto.digest(std.crypto.Algorithm.Sha256)?
    source = "!é\0☃!"
    address = "!127.0.0.1!"
    print("__test_fail_allocations_after_0__")?
    updated = digest.update_text(&source[1:4])
    parsed = std.net.parse_ip(&address[1:10])
    print("__test_restore_allocations__")?
    updated?
    drop(parsed?)
    print(True)?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True");
}

#[test]
fn sqlite_rejects_nul_inside_native_paths_but_not_outside_view() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("selected.sqlite");
    let source = r#"import std.db.sqlite

def main() -> Result[(), IoError | AllocError | std.db.sqlite.DbError]:
    path = "\0DATABASE\0"
    mut db = std.db.sqlite.connect(&path[1:-1], std.db.sqlite.Mode.Create, 4096)?
    sql = "\0SELECT 1\0"
    db.execute(&sql[1:9])?
    match std.db.sqlite.connect(&path[:-1], std.db.sqlite.Mode.Create, 4096):
        case Err(std.db.sqlite.DbError.InvalidPath):
            print(True)?
        case _:
            print(False)?
    Ok(())
"#;
    let source = source.replace("DATABASE", database.to_str().unwrap());
    assert_eq!(run_file_stdout(&source), "True");
}

#[test]
fn temporary_names_and_file_path_views_keep_native_validation() {
    let source = r#"import std.tempfile
import std.fs
import std.io
import std.path
import std.bytes

def main() -> Result[(), IoError | AllocError | std.fs.FsError | std.io.StreamError | std.bytes.BytesError]:
    decorated = "\0view-\0"
    mut folder = std.tempfile.directory(prefix=&decorated[1:6])?
    suffix = "\0.txt\0"
    mut file = std.tempfile.named_file(folder.path(), &decorated[1:6], &suffix[1:5])?
    file.write(b"ok")?
    file.flush()?
    filename = file.path().to_text()?
    wrapped = ("\0" + filename)?
    mut input = std.io.open_file(&wrapped[1:], "rb")?
    mut bytes = std.bytes.with_capacity(2)?
    bytes.resize(2, 0u8)?
    input.read(&mut bytes, 0, 2)?
    print(bytes.to_text()? == "ok")?
    input.close()?
    file.close()?
    folder.cleanup()?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True");
}

#[test]
fn asynchronous_lookup_copies_selected_host_before_return() {
    let source = r#"import std.net
import std.time

def start() -> Result[std.net.Lookup, std.net.NetError]:
    host = "\0localhost\0"
    std.net.start_lookup(&host[1:10], 8080u16)

def main() -> Result[(), IoError | std.net.NetError]:
    mut lookup = start()?
    for _ in range(1000):
        match lookup.finish_now():
            case Ok(addresses):
                print(len(addresses) > 0, addresses[0].port == 8080u16)?
                return Ok(())
            case Err(std.net.NetError.WouldBlock):
                std.time.sleep(&std.time.milliseconds(1u32))
            case Err(error):
                return Err(error)
    print(False)?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True True");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn byte_view_decoding_preserves_inline_and_fallible_heap_allocation() {
    let source = r#"import std.bytes

def main() -> Result[(), IoError | AllocError | std.bytes.BytesError]:
    source = b"!\xc3\xa9\0\xe2\x98\x83xx!"
    print("__test_fail_allocations_after_0__")?
    tiny = std.bytes.to_text(&source[1:7])
    large = std.bytes.to_text(&source[1:9])
    print("__test_restore_allocations__")?
    print(tiny? == "é\0☃")?
    match large:
        case Err(std.bytes.BytesError.Allocation(_)):
            print(True)?
        case _:
            print(False)?
    Ok(())
"#;
    assert_eq!(run_file_stdout(source), "True\nTrue");
}
