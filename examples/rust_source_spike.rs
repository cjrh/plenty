//! Developer-only driver for experiments/rust-source-backend/emitter.
use plenty::rust_source_spike::{Options, emit_file};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: rust_source_spike INPUT OUTPUT [--export name=alias] [--companion name=crate::host::name] [--helpers]")?;
    let output = args.next().ok_or("missing output")?;
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--helpers" => options.helpers = true,
            "--no-inline" => options.no_inline = true,
            "--module-size" => {
                options.module_size = args.next().ok_or("missing module size")?.parse()?
            }
            "--export" | "--companion" => {
                let pair = args.next().ok_or("missing name=target")?;
                let (name, target) = pair.split_once('=').ok_or("expected name=target")?;
                let map = if arg == "--export" {
                    &mut options.exports
                } else {
                    &mut options.companions
                };
                if map.insert(name.into(), target.into()).is_some() {
                    return Err("duplicate mapping".into());
                }
            }
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    let emission = emit_file(Path::new(&input), &options)?;
    std::fs::write(&output, &emission.source)?;
    std::fs::write(
        format!("{output}.map.json"),
        serde_json::to_vec_pretty(&emission.locations)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"check_ms":emission.check_time.as_secs_f64()*1000.0,"emit_ms":emission.emit_time.as_secs_f64()*1000.0,"rust_bytes":emission.source.len()})
    );
    Ok(())
}
