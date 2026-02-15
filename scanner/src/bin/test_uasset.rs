use std::env;
use unl_core::uasset::UAssetParser;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: test_uasset <path_to_uasset>");
        return Ok(());
    }

    let path = &args[1];
    println!("Parsing: {}", path);

    let mut parser = UAssetParser::new();
    parser.parse(path)?;

    println!("Parent Class: {:?}", parser.parent_class);

    println!("\n--- Imports ({}) ---", parser.imports.len());
    for import in &parser.imports {
        println!("Asset Ref: {}", import);
    }

    println!("\n--- Functions ({}) ---", parser.functions.len());
    for func in &parser.functions {
        println!("Function: {}", func);
    }

    Ok(())
}