use patch_builder::{hex, inspect_cia, Profile, PROFILES};
use std::{env, fs, path::Path, process::ExitCode};

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}
/// The package kind a command is for, by its name.
fn profile(name: &std::ffi::OsStr) -> Result<&'static Profile, String> {
    let name = name.to_str().ok_or("the package kind is not text")?;
    patch_builder::profile(name).map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let usage = "usage: patch-builder inspect <cia> | inspect-payload <kind> <elf> | build <kind> <code> <exheader> <elf> <fresh-output-directory>
kinds: bank, transporter, test, migration, download";
    let Some(command) = args.first().and_then(|s| s.to_str()) else {
        return Err(usage.into());
    };
    match command {
        "inspect" if args.len() == 2 => {
            let data = read(Path::new(&args[1]))?;
            let info = inspect_cia(&data).map_err(|e| e.to_string())?;
            println!("CIA bytes: {}", info.byte_len);
            println!("CIA SHA-256: {}", hex(&info.sha256));
            println!("Title ID: {:016x}", info.title_id);
            let v = info.title_version;
            println!("TMD title version: {v} ({}.{}.{}, raw encoding; not the displayed application version)", v >> 10, (v >> 4) & 63, v & 15);
            println!("Content bytes: {}", info.content_size);
            match PROFILES
                .iter()
                .map(|profile| profile.program)
                .find(|program| program.title_id == info.title_id && program.tmd_version == v)
            {
                Some(program) => println!(
                    "Title and version of {}; the build checks the exact files.",
                    program.label
                ),
                None => println!("Not a title and version the builder knows."),
            }
            for content in info.contents {
                println!(
                    "Content {:04x}: id={:08x}, type={:#06x}, bytes={}, included={}",
                    content.index,
                    content.id,
                    content.content_type,
                    content.byte_len,
                    content.file_offset.is_some()
                );
                if let Some(digest) = content.sha256 {
                    println!("  SHA-256: {}", hex(&digest));
                    println!(
                        "  TMD content hash match: {}",
                        digest == content.expected_sha256
                    );
                }
                if let Some(ncch) = content.ncch {
                    println!(
                        "  NCCH program ID: {:016x}; product: {}; format: {}; encrypted: {}",
                        ncch.program_id, ncch.product_code, ncch.format_version, ncch.encrypted
                    );
                    if let Some(header) = ncch.exheader {
                        println!(
                            "  Exheader: {}; remaster={}; compressed-code={}; SHA-256={}",
                            header.name,
                            header.remaster_version,
                            header.compressed_code,
                            hex(&header.sha256)
                        );
                        println!(
                            "  Exheader hash match: {}",
                            ncch.exheader_hash_matches.unwrap_or(false)
                        );
                        println!(
                            "  Native save bytes: {}; raw extdata descriptor: {:016x}",
                            header.native_save_size, header.extdata_id_raw
                        );
                        println!("  Raw storage access descriptor: {} (permissions require separate interpretation)", hex(&header.storage_access_raw));
                        for (name, segment) in [
                            ("text", header.text),
                            ("rodata", header.rodata),
                            ("data", header.data),
                        ] {
                            println!(
                                "  {name}: address={:#010x}, bytes={:#x}, pages={:#x}",
                                segment.address, segment.byte_len, segment.pages
                            );
                        }
                        println!(
                            "  BSS bytes: {:#x}; stack bytes: {:#x}; code image bytes: {:#x}",
                            header.bss_size,
                            header.stack_size,
                            header.code_image_size().map_err(|e| e.to_string())?
                        );
                        println!("  Services: {}", header.services.join(", "));
                    }
                    if let Some(code) = ncch.code {
                        println!(
                            "  Stored .code bytes: {}; SHA-256: {}; ExeFS hash match: {}",
                            code.byte_len,
                            hex(&code.sha256),
                            code.sha256 == code.expected_sha256
                        );
                    }
                }
            }
            println!("Signature authenticity is not verified. No patch was generated.");
        }
        "inspect-payload" if args.len() == 3 => {
            let profile = profile(&args[1])?;
            let data = read(Path::new(&args[2]))?;
            let info = patch_builder::elf::inspect_payload_elf(
                profile.program.contract,
                &data,
                &profile.entries(),
            )
            .map_err(|e| e.to_string())?;
            println!("ELF SHA-256: {}", hex(&info.elf_sha256));
            println!(
                "Bootstrap: {:#010x}; bytes={:#x}",
                info.bootstrap_entry,
                info.bootstrap_bytes().len()
            );
            println!(
                "Payload: {:#010x}; initialized={:#x}; RX={:#x}; total={:#x}",
                profile.program.contract.payload,
                info.image_bytes().len(),
                info.executable_size,
                info.memory_size
            );
            for (name, address) in &info.entries {
                println!("Export: {name}={address:#010x}");
            }
            println!("ELF structure verified; the edits of the original are checked by build.");
        }
        "build" if args.len() == 6 => {
            let profile = profile(&args[1])?;
            let code = read(Path::new(&args[2]))?;
            let exheader = read(Path::new(&args[3]))?;
            let elf = read(Path::new(&args[4]))?;
            let built = patch_builder::build(profile, &code, &exheader, &elf, Path::new(&args[5]))
                .map_err(|error| error.to_string())?;
            println!(
                "{} package ({}): {}",
                profile.program.label,
                profile.name,
                built.directory.display()
            );
            println!("code.ips SHA-256: {}", hex(&built.ips_sha256));
            println!("exheader.bin SHA-256: {}", hex(&built.exheader_sha256));
            println!(
                "Expanded code SHA-256: {}",
                hex(&built.expanded_code_sha256)
            );
            println!("Paired files, report.md and manifest.json written. Nothing installed.");
        }
        "--help" | "-h" if args.len() == 1 => println!("{usage}"),
        _ => return Err(usage.into()),
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("patch-builder: {error}");
            ExitCode::FAILURE
        }
    }
}
