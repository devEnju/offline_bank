use patch_builder::{hex, inspect_cia, inspect_code, inspect_exheader, BANK_TITLE_ID};
use std::{env, fs, path::Path, process::ExitCode};

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let usage = "usage: patch-builder inspect <cia> | inspect-code <decompressed-code> [exheader] | inspect-payload <elf> | check-development <code> <exheader> <elf> | build-development <code> <exheader> <elf> <fresh-output-directory> | build-transporter <code> <exheader> <elf> <fresh-output-directory>";
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
            if info.title_id == BANK_TITLE_ID && v == 0 {
                println!(
                    "Status: original Bank version 0; insufficient for the planned Bank 1.5 patch."
                );
            } else {
                println!("Status: identity recorded; patch compatibility requires the exact development-profile fingerprints.");
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
        "inspect-code" if args.len() == 2 || args.len() == 3 => {
            let data = read(Path::new(&args[1]))?;
            let header = args
                .get(2)
                .map(|path| {
                    read(Path::new(path))
                        .and_then(|bytes| inspect_exheader(&bytes).map_err(|e| e.to_string()))
                })
                .transpose()?;
            let info = inspect_code(&data, header.as_ref()).map_err(|e| e.to_string())?;
            println!("Code bytes: {} ({:#x})", info.byte_len, info.byte_len);
            println!("Code SHA-256: {}", hex(&info.sha256));
            if let Some(length) = info.expected_length {
                println!("Exheader page total: {length}; size matches.");
            } else {
                println!("No exheader supplied: decompression and mapped length were not checked.");
            }
            println!("Code inspection alone does not validate or generate an offline patch.");
        }
        "inspect-payload" if args.len() == 2 => {
            let data = read(Path::new(&args[1]))?;
            let info = patch_builder::elf::inspect_payload_elf(&data).map_err(|e| e.to_string())?;
            println!("ELF SHA-256: {}", hex(&info.elf_sha256));
            println!(
                "Bootstrap: {:#010x}; bytes={:#x}",
                info.bootstrap_entry,
                info.bootstrap_bytes().len()
            );
            println!(
                "Payload: {:#010x}; initialized={:#x}; RX={:#x}; total={:#x}",
                patch_builder::bank15::PAYLOAD_ADDRESS,
                info.image_bytes().len(),
                info.executable_size,
                info.memory_size
            );
            println!(
                "Exports: next={:#010x}, load={:#010x}, save={:#010x}, validate-game={:#010x}, rewards={:#010x}, timestamp={:#010x}",
                info.entry, info.load_entry, info.save_entry, info.validate_game_entry, info.rewards_entry, info.timestamp_entry
            );
            println!(
                "Dex hooks: request={:#010x}, update={:#010x}, finish={:#010x}",
                info.dex_save_request_entry,
                info.dex_records_update_entry,
                info.dex_records_finish_entry
            );
            println!("ELF structure verified. Runtime behavior and native hook installation are not approved by this check.");
        }
        "check-development" if args.len() == 4 => {
            let code = read(Path::new(&args[1]))?;
            let exheader = read(Path::new(&args[2]))?;
            let elf = read(Path::new(&args[3]))?;
            let prepared = patch_builder::prepare_development_patch(&code, &exheader, &elf)
                .map_err(|e| e.to_string())?;
            println!("Development profile: all original bytes, hashes, ELF exports, and placement checks passed.");
            println!("ELF SHA-256: {}", hex(&patch_builder::sha256(&elf)));
            println!(
                "In-memory IPS bytes: {}; exheader bytes: {}",
                prepared.ips().len(),
                prepared.exheader().len()
            );
            println!(
                "Expanded code SHA-256: {}",
                hex(&prepared.expanded_code_sha256())
            );
            println!(
                "Expanded image bytes: {:#x}",
                prepared.layout().expanded_code_size
            );
            println!("No files written. Runtime review and console validation remain required.");
        }
        "build-development" if args.len() == 5 => {
            let code = read(Path::new(&args[1]))?;
            let exheader = read(Path::new(&args[2]))?;
            let elf = read(Path::new(&args[3]))?;
            let built =
                patch_builder::build_development_patch(&code, &exheader, &elf, Path::new(&args[4]))
                    .map_err(|error| error.to_string())?;
            println!("Development artifacts: {}", built.directory.display());
            println!("code.ips SHA-256: {}", hex(&built.ips_sha256));
            println!("exheader.bin SHA-256: {}", hex(&built.exheader_sha256));
            println!(
                "Expanded code SHA-256: {}",
                hex(&built.expanded_code_sha256)
            );
            println!(
                "Paired files, manifest.json, and report.md written. No deployment performed."
            );
            println!("Runtime behavior and save recovery require console validation.");
        }
        "build-transporter" if args.len() == 5 => {
            let code = read(Path::new(&args[1]))?;
            let exheader = read(Path::new(&args[2]))?;
            let elf = read(Path::new(&args[3]))?;
            let built =
                patch_builder::transporter15::build(&code, &exheader, &elf, Path::new(&args[4]))
                    .map_err(|error| error.to_string())?;
            println!("Transporter artifacts: {}", built.directory.display());
            println!("code.ips SHA-256: {}", hex(&built.ips_sha256));
            println!("exheader.bin SHA-256: {}", hex(&built.exheader_sha256));
            println!(
                "Expanded code SHA-256: {}",
                hex(&built.expanded_code_sha256)
            );
            println!(
                "Paired files, manifest.json, and report.md written. No deployment performed."
            );
            println!("Behavior on a console is not established by this build.");
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
